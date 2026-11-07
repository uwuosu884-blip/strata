//! Corpus (Live 12): MIDI Input strip / column, resonator display (type,
//! quality, 3D icon with the hit point, Decay × Material / Radius XY, Width /
//! Pos. L / Pos. R), Bright / Inharm (Opening) / Ratio / Hit, Tune (note
//! box) / Fine / Spread, LFO, band-pass Filter, Bleed / Gain (limiter LED) /
//! Dry/Wet. Geometry measured from Live 12 at 100 % (l/t = device left/top;
//! text positions are baselines).

use super::corpus_icons::{HIT_C, ICONS};
use super::dynamics::{KN, Kn, knob_lv, knob12, sig3, tbutton};
use super::fx4::*;
use super::fx5::*;
use super::modfx::{drag_text, small_tri, text_menu};
use super::shifter::{ct, sync_btn};
use super::*;
use daw_engine::modfx::SYNC_DIVS;
use daw_project::devices::corpus::*;
use daw_project::devices::params6::cop;

/// Width of the unfolded MIDI Input column.
pub const MIDI_W: f32 = 88.0;
/// LFO knobs (smaller ring).
const KN_LFO: Kn = Kn { r: 8.2, w: 2.0, span: 150.0 * std::f32::consts::PI / 180.0 };

pub fn width(d: &CorpusParams) -> f32 {
    if d.open { 520.0 + MIDI_W } else { 520.0 }
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut CorpusParams)) {
    if let FxParams::Corpus(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn div(i: u8) -> &'static str {
    SYNC_DIVS[(i as usize).min(SYNC_DIVS.len() - 1)].1
}

fn pct(v: f32) -> String {
    format!("{:.1} %", v * 100.0)
}

/// Live note name (C3 = MIDI 60) and cents of `hz`.
fn note_ct(hz: f32) -> (String, i32) {
    let m = 69.0 + 12.0 * (hz.max(1.0) / 440.0).log2();
    let n = m.round();
    let names = daw_project::clipx::NOTE_NAMES;
    let i = (n as i32).rem_euclid(12) as usize;
    (format!("{}{}", names[i], (n as i32).div_euclid(12) - 2), ((m - n) * 100.0).round() as i32)
}

/// Centred draggable text (enabled → `col`, else grey).
#[allow(clippy::too_many_arguments)]
fn drag_ce(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, s: &str, en: bool, col: Color32) {
    if !en {
        ct(p, x, base, s, gray(0x86));
        return;
    }
    let w = p.layout_no_wrap(s.to_string(), font(FS), col).size().x;
    drag_text(cx, ui, p, x - w / 2.0, base, k, s, en, col);
}

/// 9×9 section checkbox (#1E, orange when on) with a label.
#[allow(clippy::too_many_arguments)]
fn chk(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, x: f32, k: u8, label: &str, on: bool) -> bool {
    let b = rr(l, t, x, 22.0, x + 9.0, 31.0);
    p.rect_filled(b, CornerRadius::ZERO, gray(0x11));
    p.rect_filled(b.shrink(1.0), CornerRadius::ZERO, if on { if cx.on { ORANGE } else { gray(0x75) } } else { gray(0x1e) });
    let tr = text_at(p, l + x + 13.0, t + 31.0, Align2::LEFT_BOTTOM, label, font(FS), text());
    cx.toggle(ui, Rect::from_min_max(b.min - vec2(2.0, 2.0), pos2(tr.right() + 2.0, b.bottom() + 2.0)), k, true)
}

/// Small knob with the label above-left and the value below-right (LFO).
#[allow(clippy::too_many_arguments)]
fn knob_s(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, base: f32, cy: f32, k: u8, label: &str, value: &str, en: bool) {
    text_at(p, l + 319.0, t + base, Align2::LEFT_BOTTOM, label, font(FS), text());
    let area = rr(l, t, 316.0, base - 9.0, 362.0, cy + 12.0);
    cx.drag(ui, area, k, en);
    let v = norm(&cx.params, k).unwrap_or(0.0);
    knob12(p, pos2(l + 328.5, t + cy), v, 0.0, KN_LFO, en && cx.on, cx.modv(k));
    text_at(p, l + 331.0, t + cy + 11.5, Align2::LEFT_BOTTOM, value, font(FS), text());
    if cx.focused(k) {
        brackets(p, area);
    }
}

pub(super) fn title_logo(p: &Painter, r: Rect) {
    let (x, t) = (r.right() - 77.0, r.top());
    for (i, s) in ["A", "A", "S"].iter().enumerate() {
        text_at(p, x + 4.5 + 10.0 * i as f32, t + 13.0, Align2::CENTER_BOTTOM, s, bold(FS), gray(0x16));
    }
    for i in 1..3 {
        px(p, x, t, 10.0 * i as f32 - 1.0, 4.0, 10.0 * i as f32, 13.0, gray(0x24));
    }
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Corpus(d) = cx.params else { return };
    let (l0, t) = (r.left(), r.top());
    strip(cx, ui, p, l0, t, &d);
    if d.open {
        midi_column(cx, ui, p, l0, t, &d);
    }
    let l = l0 + if d.open { MIDI_W } else { 0.0 };
    let tube = d.is_tube();
    let lab = gray(0x86);
    let acc = if cx.on { ORANGE } else { gray(0x75) };

    // Panel dividers (#46, 2 px).
    for x in [311.0, 404.0, 463.0] {
        px(p, l, t, x, 21.0, x + 2.0, 188.0, gray(0x46));
    }

    // Display.
    p.rect_filled(rr(l, t, 21.0, 21.0, 243.0, 131.0), CornerRadius::same(2), gray(0x18));
    px(p, l, t, 188.0, 21.0, 189.0, 131.0, gray(0x24));
    let types: Vec<&str> = CO_TYPES.to_vec();
    if let Some(i) = text_menu(
        cx,
        ui,
        p,
        rr(l, t, 24.0, 22.0, 129.0, 33.0),
        cop::KIND,
        l + 26.0,
        t + 31.0,
        Some(l + 122.0),
        d.kind as usize,
        &types,
        true,
        ORANGE,
    ) {
        set(cx, |q| q.kind = i as u8);
    }
    px(p, l, t, 132.0, 22.0, 133.0, 31.0, gray(0x46));
    let qual: Vec<&str> = CO_QUALITY.to_vec();
    if tube {
        text_at(p, l + 139.0, t + 31.0, Align2::LEFT_BOTTOM, qual[d.quality as usize % 4], font(FS), gray(0x86));
        small_tri(p, l + 177.0, t + 27.0, gray(0x86));
    } else if let Some(i) = text_menu(
        cx,
        ui,
        p,
        rr(l, t, 136.0, 22.0, 185.0, 33.0),
        cop::QUALITY,
        l + 139.0,
        t + 31.0,
        Some(l + 177.0),
        d.quality as usize,
        &qual,
        true,
        ORANGE,
    ) {
        set(cx, |q| q.quality = i as u8);
    }
    // Icon + hit point (moves from the centre toward the edge along the axis).
    let ic = &ICONS[(d.kind as usize).min(6)];
    let h = if tube { 0.5 } else { d.hit };
    let hc = pos2(l + HIT_C.0 + (h - 0.5) * 14.0, t + HIT_C.1 - (h - 0.5) * 7.0);
    let base = pos2(l + HIT_C.0 + ic.dx, t + HIT_C.1 + ic.dy).round();
    let mut m = egui::Mesh::default();
    for (j, row) in ic.a.chunks(ic.w).enumerate() {
        for (i, &a) in row.iter().enumerate() {
            if a > 0 {
                let q = Rect::from_min_size(base + vec2(i as f32, j as f32), vec2(1.0, 1.0));
                m.add_colored_rect(q, gray(0x90).gamma_multiply(a as f32 / 255.0));
            }
        }
    }
    p.add(Shape::mesh(m));
    p.circle_stroke(hc, 5.6, Stroke::new(1.5, acc));
    // XY: Decay (x) × Material / Radius (y).
    let ky = if tube { cop::RADIUS } else { cop::MATERIAL };
    xy_drag(cx, ui, rr(l, t, 21.0, 36.0, 188.0, 113.0), cop::DECAY, ky, None, 167.0, 77.0);
    text_at(p, l + 26.0, t + 124.0, Align2::LEFT_BOTTOM, "Decay", font(FS), lab);
    drag_text(cx, ui, p, l + 60.5, t + 124.0, cop::DECAY, &format!("{} s", sig3(d.decay)), true, ORANGE);
    let (yl, yv) = if tube { ("Radius", format!("{:.0} %", d.radius * 100.0)) } else { ("Material", pct(d.material)) };
    text_at(p, l + 143.0, t + 124.0, Align2::RIGHT_BOTTOM, yl, font(FS), lab);
    cx.salt = 1;
    drag_text(cx, ui, p, l + 147.0, t + 124.0, ky, &yv, true, ORANGE);
    cx.salt = 0;
    // Width / Pos. L / Pos. R.
    for (i, (k, name, v, en)) in
        [(cop::WIDTH, "Width", d.width, true), (cop::POS_L, "Pos. L", d.pos_l, !tube), (cop::POS_R, "Pos. R", d.pos_r, !tube)]
            .into_iter()
            .enumerate()
    {
        let b = 30.5 + 40.0 * i as f32;
        ct(p, l + 216.5, t + b, name, lab);
        drag_ce(cx, ui, p, l + 216.5, t + b + 13.5, k, &format!("{:.0} %", v * 100.0), en, ORANGE);
    }

    // Bright / Inharm (Opening) / Ratio / Hit.
    knob_lv(cx, ui, p, l + 48.0, t + 145.0, cop::BRIGHT, "Bright", &pct(d.bright), 0.0, KN, 13.0, 35.0, !tube);
    if d.kind == 5 {
        knob_lv(
            cx,
            ui,
            p,
            l + 104.0,
            t + 145.0,
            cop::OPENING,
            "Opening",
            &format!("{:.0} %", d.opening * 100.0),
            0.0,
            KN,
            13.0,
            35.0,
            true,
        );
    } else {
        knob_lv(cx, ui, p, l + 104.0, t + 145.0, cop::INHARM, "Inharm", &pct(d.inharm), 0.0, KN, 13.0, 35.0, !tube);
    }
    knob_lv(cx, ui, p, l + 160.0, t + 145.0, cop::RATIO, "Ratio", &format!("{:.0} %", d.ratio * 100.0), 0.0, KN, 13.0, 35.0, d.has_ratio());
    knob_lv(cx, ui, p, l + 214.0, t + 145.0, cop::HIT, "Hit", &format!("{:.0} %", d.hit * 100.0), 0.0, KN, 13.0, 35.0, !tube);
    px(p, l, t, 243.0, 136.0, 244.0, 182.0, gray(0x24));

    // Tune (Transpose with MIDI Frequency) / note box / Fine / Spread.
    let midi_f = d.midi_freq && d.midi.on && d.midi.source != 0;
    if midi_f {
        knob_lv(cx, ui, p, l + 277.0, t + 31.0, cop::TRANSPOSE, "Transpose", &st0(d.transpose), 0.5, KN, 13.5, 35.5, true);
    } else {
        knob_lv(cx, ui, p, l + 277.0, t + 31.0, cop::TUNE, "Tune", &format!("{:.2}", d.tune), 0.0, KN, 13.5, 35.5, true);
    }
    let nb = rr(l, t, 247.0, 73.0, 308.0, 88.0);
    p.rect_filled(nb, CornerRadius::same(2), gray(0x46));
    if !midi_f {
        let (n, c) = note_ct(d.tune);
        let tc = gray(0x12);
        text_at(p, l + 255.0, t + 85.0, Align2::LEFT_BOTTOM, &n, font(FS), tc);
        text_at(p, l + 285.0, t + 85.0, Align2::RIGHT_BOTTOM, &format!("{c}"), font(FS), tc);
        text_at(p, l + 292.0, t + 85.0, Align2::LEFT_BOTTOM, "ct", font(FS), tc);
    }
    knob_lv(cx, ui, p, l + 277.0, t + 105.0, cop::FINE, "Fine", &format!("{:.0} ct", d.fine), 0.5, KN, 19.0, 41.5, midi_f);
    marker(p, l + 277.0, t + 124.0, marker_col(cx, midi_f, d.fine.abs() >= 0.5));
    ct(p, l + 277.5, t + 162.5, "Spread", text());
    value_box(cx, ui, p, rr(l, t, 248.0, 167.0, 307.0, 180.0), cop::SPREAD, &pct(d.spread), true);

    // LFO.
    if chk(cx, ui, p, l, t, 318.0, cop::LFO_ON, "LFO", d.lfo_on) {
        set(cx, |q| q.lfo_on = !q.lfo_on);
    }
    let lo = d.lfo_on;
    knob_s(cx, ui, p, l, t, 56.0, 70.0, cop::LFO_AMT, "Amount", &pct(d.lfo_amt), lo);
    if d.lfo_sync {
        knob_s(cx, ui, p, l, t, 105.0, 119.0, cop::DIV, "Rate", div(d.div), lo);
    } else {
        knob_s(cx, ui, p, l, t, 105.0, 119.0, cop::RATE, "Rate", &format!("{:.2}", d.rate), lo);
    }
    if d.lfo_sync {
        knob_s(cx, ui, p, l, t, 154.0, 168.0, cop::OFFSET, "Offset", &format!("{:.0}°", d.offset * 360.0), lo);
    } else if d.spin_mode {
        knob_s(cx, ui, p, l, t, 154.0, 168.0, cop::SPIN, "Spin", &pct(d.spin), lo);
    } else {
        knob_s(cx, ui, p, l, t, 154.0, 168.0, cop::PHASE, "Phase", &format!("{:.0}°", d.phase * 360.0), lo);
    }
    text_at(p, l + 368.0, t + 56.0, Align2::LEFT_BOTTOM, "Shape", font(FS), text());
    let sr = rr(l, t, 368.0, 64.0, 398.0, 76.0);
    let waves: Vec<&str> = CO_LFO_WAVES.to_vec();
    if let Some(i) = chooser(cx, ui, p, sr, cop::WAVE, "", &waves, lo) {
        set(cx, |q| q.lfo_wave = i as u8);
    }
    let wc = if lo && cx.on { text() } else { dim() };
    let rnd = [0.6, -0.4, 0.9, -0.8];
    let pts: Vec<Pos2> = (0..=16)
        .map(|i| {
            let u = i as f32 / 16.0;
            let n = (u * 3.99) as usize;
            let v = co_wave(d.lfo_wave, u, (rnd[n], rnd[(n + 1) % 4]));
            pos2(sr.left() + 3.0 + u * 16.0, sr.center().y - v * 3.5)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.2, wc)));
    let line = gray(0x4a);
    px(p, l, t, 362.0, 115.0, 373.0, 116.0, line);
    px(p, l, t, 362.0, 163.0, 373.0, 164.0, line);
    if sel_btn(cx, ui, p, rr(l, t, 380.0, 101.0, 398.0, 114.0), cop::LFO_SYNC, "Hz", !d.lfo_sync, lo) {
        set(cx, |q| q.lfo_sync = false);
    }
    cx.salt = 1;
    if sync_btn(cx, ui, p, rr(l, t, 380.0, 117.0, 398.0, 130.0), cop::LFO_SYNC, d.lfo_sync, lo) {
        set(cx, |q| q.lfo_sync = true);
    }
    cx.salt = 0;
    let pe = lo && !d.lfo_sync;
    for (i, (y, sel)) in [(150.0, !d.spin_mode), (166.0, d.spin_mode)].into_iter().enumerate() {
        let b = rr(l, t, 380.0, y, 398.0, y + 13.0);
        cx.salt = i as u32;
        if sel_btn(cx, ui, p, b, cop::SPIN_MODE, "", sel, pe) && !sel {
            set(cx, |q| q.spin_mode = i == 1);
        }
        let ic = match (sel, pe && cx.on) {
            (true, true) => gray(0x20),
            (true, false) => gray(0x36),
            (false, true) => text(),
            (false, false) => dim(),
        };
        let c = b.center();
        if i == 0 {
            p.circle_stroke(c, 3.0, Stroke::new(1.1, ic));
            p.line_segment([pos2(c.x, c.y - 4.5), pos2(c.x, c.y + 4.5)], Stroke::new(1.1, ic));
        } else {
            p.circle_stroke(c, 4.0, Stroke::new(1.1, ic));
            p.circle_filled(pos2(c.x - 1.2, c.y), 1.6, ic);
        }
    }
    cx.salt = 0;

    // Filter.
    if chk(cx, ui, p, l, t, 411.0, cop::FILTER_ON, "Filter", d.filter_on) {
        set(cx, |q| q.filter_on = !q.filter_on);
    }
    knob_lv(cx, ui, p, l + 433.5, t + 87.5, cop::FREQ, "Freq", &hz(d.freq), 0.0, KN, 17.5, 35.0, d.filter_on);
    knob_lv(cx, ui, p, l + 433.5, t + 145.0, cop::BW, "Bdwidth", &format!("{:.2}", d.bw), 0.0, KN, 15.0, 35.0, d.filter_on);

    // Bleed / Gain + limiter LED / Dry/Wet.
    knob_lv(cx, ui, p, l + 493.0, t + 31.0, cop::BLEED, "Bleed", &pct(d.bleed), 0.0, KN, 14.0, 35.5, !tube);
    knob_lv(cx, ui, p, l + 493.0, t + 88.0, cop::GAIN, "Gain", &format!("{:.1} dB", d.gain), 0.0, KN, 17.0, 35.0, true);
    let lim = cx.on && dev_tap(cx.dev.id).get(TAP_LIMIT) > 0.5;
    p.circle_filled(pos2(l + 512.0, t + 83.5), 3.5, if lim { Color32::from_rgb(0xff, 0x5a, 0x3a) } else { gray(0x18) });
    if lim {
        cx.app.animating = true;
    }
    knob_lv(cx, ui, p, l + 493.0, t + 145.0, cop::DRY_WET, "Dry/Wet", &format!("{:.0} %", d.dry_wet * 100.0), 0.0, KN, 15.0, 35.0, true);
}

/// Folded MIDI Input strip: fold button, vertical caption, on LED.
fn strip(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &CorpusParams) {
    px(p, l, t, 19.0, 21.0, 21.0, 188.0, gray(0x46));
    let fc = pos2(l + 9.5, t + 27.5);
    p.circle_filled(fc, 6.5, gray(0x12));
    p.circle_filled(fc, 5.5, gray(0x1c));
    let tri = if d.open {
        vec![pos2(l + 7.0, t + 24.0), pos2(l + 13.5, t + 27.5), pos2(l + 7.0, t + 31.0)]
    } else {
        vec![pos2(l + 6.0, t + 25.0), pos2(l + 13.0, t + 25.0), pos2(l + 9.5, t + 31.5)]
    };
    p.add(Shape::convex_polygon(tri, gray(0xb5), Stroke::NONE));
    if cx.toggle(ui, Rect::from_center_size(fc, vec2(13.0, 13.0)), cop::OPEN, true) {
        set(cx, |q| q.open = !q.open);
    }
    let gal = p.layout_no_wrap("MIDI Input".into(), font(FS), text());
    let asc = gal.rows.first().map_or(8.0, |r| r.row.glyphs.first().map_or(8.0, |gl| gl.pos.y));
    p.add(egui::epaint::TextShape::new(pos2(l + 13.0 - asc, t + 171.0), gal, text()).with_angle(-std::f32::consts::FRAC_PI_2));
    let lc = pos2(l + 9.5, t + 178.5);
    let lit = d.midi.on && cx.on;
    p.circle_filled(lc, 3.5, if lit { ORANGE } else { gray(0x24) });
    if cx.toggle(ui, Rect::from_center_size(lc, vec2(11.0, 11.0)), cop::MIDI_ON, true) {
        set(cx, |q| q.midi.on = !q.midi.on);
    }
}

/// Unfolded MIDI Input column: MIDI From, Frequency (Last / Low), PB Range,
/// Off Decay.
fn midi_column(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &CorpusParams) {
    px(p, l, t, 107.0, 21.0, 109.0, 188.0, gray(0x46));
    text_at(p, l + 27.0, t + 30.0, Align2::LEFT_BOTTOM, "MIDI From", font(FS), text());
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
    let cur = tracks.iter().find(|x| x.0 == d.midi.source).map_or("No Input".to_string(), |x| x.1.clone());
    let mut items: Vec<&str> = vec!["No Input"];
    items.extend(tracks.iter().map(|x| x.1.as_str()));
    let cr = rr(l, t, 26.0, 34.0, 101.0, 47.0);
    let shown = fit(p, &cur, &font(FS), cr.width() - 14.0);
    if let Some(i) = chooser(cx, ui, p, cr, cop::SOURCE, &shown, &items, true) {
        let src = if i == 0 { 0 } else { tracks[i - 1].0 };
        set(cx, |q| {
            q.midi.source = src;
            q.midi.on = src != 0;
        });
    }
    let ext = d.midi.source != 0;
    let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
    let tr = rr(l, t, 26.0, 50.0, 101.0, 63.0);
    if !ext {
        boxed(p, tr, box_fill(cx.on));
    } else if let Some(i) = chooser(cx, ui, p, tr, cop::TAP, d.midi.tap.label(), &taps, true) {
        set(cx, |q| q.midi.tap = ScTap::ALL[i]);
    }
    if tbutton(cx, ui, p, rr(l, t, 26.0, 70.0, 101.0, 83.0), cop::MIDI_FREQ, "Frequency", d.midi_freq, true) {
        set(cx, |q| q.midi_freq = !q.midi_freq);
    }
    if sel_btn(cx, ui, p, rr(l, t, 26.0, 86.0, 62.0, 99.0), cop::LOW, "Last", !d.low, d.midi_freq) {
        set(cx, |q| q.low = false);
    }
    cx.salt = 1;
    if sel_btn(cx, ui, p, rr(l, t, 65.0, 86.0, 101.0, 99.0), cop::LOW, "Low", d.low, d.midi_freq) {
        set(cx, |q| q.low = true);
    }
    cx.salt = 0;
    text_at(p, l + 27.0, t + 116.0, Align2::LEFT_BOTTOM, "PB", font(FS), text());
    value_box_ex(cx, ui, p, rr(l, t, 56.0, 106.0, 101.0, 119.0), cop::PB, &format!("{:.0} st", d.pb), d.midi_freq, false, box_fill(cx.on));
    if tbutton(cx, ui, p, rr(l, t, 26.0, 150.0, 101.0, 163.0), cop::OFF_DECAY, "Off Decay", d.off_decay, true) {
        set(cx, |q| q.off_decay = !q.off_decay);
    }
    value_box(cx, ui, p, rr(l, t, 26.0, 169.0, 101.0, 182.0), cop::OFF_AMT, &format!("{:.0} %", d.off_amt * 100.0), d.off_decay);
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Corpus(CorpusParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    if let Some(v) = norm(p, k) {
        let n = match k {
            cop::TRANSPOSE => 2.0 * CO_TRANSPOSE,
            cop::PB => 24.0,
            cop::DIV => (SYNC_DIVS.len() - 1) as f32,
            _ => 100.0,
        };
        set_norm(p, k, ((v * n).round() + dir) / n);
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        cop::DECAY => "Decay",
        cop::MATERIAL => "Material",
        cop::RADIUS => "Radius",
        cop::BRIGHT => "Brightness",
        cop::INHARM => "Inharmonics",
        cop::OPENING => "Opening",
        cop::RATIO => "Ratio",
        cop::HIT => "Hit",
        cop::WIDTH => "Width",
        cop::POS_L => "Listening L",
        cop::POS_R => "Listening R",
        cop::TUNE => "Tune",
        cop::TRANSPOSE => "Transpose",
        cop::FINE => "Fine",
        cop::SPREAD => "Spread",
        cop::LFO_AMT => "LFO Amount",
        cop::RATE => "LFO Rate",
        cop::DIV => "LFO Sync Rate",
        cop::PHASE => "LFO Phase",
        cop::SPIN => "LFO Spin",
        cop::OFFSET => "LFO Offset",
        cop::FREQ => "Filter Freq",
        cop::BW => "Filter Bandwidth",
        cop::BLEED => "Bleed",
        cop::GAIN => "Gain",
        cop::DRY_WET => "Dry/Wet",
        cop::PB => "PB Range",
        cop::OFF_AMT => "Off Decay",
        cop::KIND => "Resonance Type",
        cop::QUALITY => "Quality",
        cop::LFO_ON => "LFO On",
        cop::WAVE => "LFO Shape",
        cop::LFO_SYNC => "LFO Sync",
        cop::SPIN_MODE => "LFO Stereo Mode",
        cop::FILTER_ON => "Filter On",
        cop::MIDI_FREQ => "MIDI Frequency",
        cop::LOW => "Note Priority",
        cop::OFF_DECAY => "Off Decay On",
        cop::SOURCE => "MIDI From",
        cop::TAP => "MIDI From Tap",
        cop::MIDI_ON => "Sidechain",
        _ => "",
    }
}
