//! Shifter (Live 12): MIDI Input strip / column, Coarse / Fine (Spread),
//! Wide, Window / Tone, Delay + Feedback, the LFO display with Duty and
//! Phase / Spin, the Env Fol display, Mode / Output and the LFO / Env Fol /
//! Dry/Wet row. Geometry measured from Live 12 at 100 % (l/t = device
//! left/top; text positions are baselines).

use super::dynamics::{KN, KN_BIG, knob_lv, sig3, tbutton};
use super::fx4::*;
use super::fx5::*;
use super::modfx::drag_text;
use super::*;
use daw_engine::modfx::SYNC_DIVS;
use daw_project::devices::params5::shp;
use daw_project::devices::shifter::*;

/// Width of the unfolded MIDI Input column.
pub const MIDI_W: f32 = 88.0;

pub fn width(d: &ShifterParams) -> f32 {
    if d.midi_open { 396.0 + MIDI_W } else { 396.0 }
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut ShifterParams)) {
    if let FxParams::Shifter(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn hz(f: f32) -> String {
    if f.abs() >= 999.5 { format!("{} kHz", sig3(f / 1000.0)) } else { format!("{} Hz", sig3(f)) }
}

fn div(i: u8) -> &'static str {
    SYNC_DIVS[(i as usize).min(SYNC_DIVS.len() - 1)].1
}

/// Centred text.
pub(super) fn ct(p: &Painter, x: f32, base: f32, s: &str, col: Color32) {
    text_at(p, x, base, Align2::CENTER_BOTTOM, s, font(FS), col);
}

/// Centred draggable text.
#[allow(clippy::too_many_arguments)]
pub(super) fn drag_c(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, s: &str, col: Color32) {
    let w = p.layout_no_wrap(s.to_string(), font(FS), col).size().x;
    drag_text(cx, ui, p, x - w / 2.0, base, k, s, true, col);
}

/// Box with the metronome glyph (synced time mode).
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_btn(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, sel: bool, en: bool) -> bool {
    let hit = if en { tbutton(cx, ui, p, r, k, "", sel, true) } else { sel_btn(cx, ui, p, r, k, "", sel, false) };
    let col = match (sel, en && cx.on) {
        (true, true) => gray(0x20),
        (true, false) => gray(0x36),
        (false, true) => text(),
        (false, false) => dim(),
    };
    glyph(p, r.left() + 6.0, r.top() + 2.0, &METRO, col);
    hit
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Shifter(d) = cx.params else { return };
    let (l0, t) = (r.left(), r.top());
    strip(cx, ui, p, l0, t, &d);
    if d.midi_open {
        midi_column(cx, ui, p, l0, t, &d);
    }
    let l = l0 + if d.midi_open { MIDI_W } else { 0.0 };
    let pitch = d.mode == 0;
    let amt_unit = |v: f32| if pitch { format!("{} st", sig3(v * SH_AMT_ST)) } else { hz(v * SH_AMT_HZ) };

    // Coarse / Fine (Spread when Wide) / Wide.
    if pitch {
        knob_lv(cx, ui, p, l + 50.0, t + 30.0, shp::COARSE, "Coarse", &st0(d.coarse), 0.0, KN_BIG, 23.5, 51.0, true);
    } else {
        knob_lv(cx, ui, p, l + 50.0, t + 30.0, shp::COARSE_HZ, "Coarse", &hz(d.coarse_hz), 0.0, KN_BIG, 23.5, 51.0, true);
    }
    let (fk, fl, fv) = if d.wide { (shp::SPREAD, "Spread", d.spread) } else { (shp::FINE, "Fine", d.fine) };
    knob_lv(cx, ui, p, l + 101.5, t + 30.0, fk, fl, &format!("{fv:.0} ct"), 0.5, KN, 21.5, 42.0, true);
    marker(p, l + 101.5, t + 51.5, marker_col(cx, true, fv.abs() >= 0.5));
    if tbutton(cx, ui, p, rr(l, t, 78.0, 76.0, 125.0, 89.0), shp::WIDE, "Wide", d.wide, true) {
        set(cx, |q| q.wide = !q.wide);
    }

    // Window / Tone.
    ct(p, l + 49.5, t + 105.0, "Window", text());
    value_box(cx, ui, p, rr(l, t, 26.0, 109.0, 73.0, 122.0), shp::WINDOW, &format!("{} ms", sig3(d.window)), pitch);
    ct(p, l + 101.5, t + 105.0, "Tone", text());
    value_box(cx, ui, p, rr(l, t, 78.0, 109.0, 125.0, 122.0), shp::TONE, &hz(d.tone), true);

    // Delay: on, time, ms / sync; Feedback.
    if tbutton(cx, ui, p, rr(l, t, 26.0, 132.0, 73.0, 145.0), shp::DELAY_ON, "Delay", d.delay_on, true) {
        set(cx, |q| q.delay_on = !q.delay_on);
    }
    let dr = rr(l, t, 26.0, 150.0, 73.0, 163.0);
    if d.delay_sync {
        value_box_ex(cx, ui, p, dr, shp::DELAY_DIV, div(d.delay_div), true, false, box_fill(cx.on));
    } else {
        value_box_ex(cx, ui, p, dr, shp::DELAY_MS, &ms(d.delay_ms), true, false, box_fill(cx.on));
    }
    if sel_btn(cx, ui, p, rr(l, t, 26.0, 169.0, 48.0, 182.0), shp::DELAY_SYNC, "ms", !d.delay_sync, d.delay_on) {
        set(cx, |q| q.delay_sync = false);
    }
    cx.salt = 1;
    if sync_btn(cx, ui, p, rr(l, t, 51.0, 169.0, 73.0, 182.0), shp::DELAY_SYNC, d.delay_sync, d.delay_on) {
        set(cx, |q| q.delay_sync = true);
    }
    cx.salt = 0;
    knob_lv(
        cx,
        ui,
        p,
        l + 102.5,
        t + 142.0,
        shp::FEEDBACK,
        "Feedback",
        &format!("{:.1} %", d.feedback * 100.0),
        0.0,
        KN,
        14.5,
        36.0,
        d.delay_on,
    );

    // Dividers.
    for (x, y0) in [(131.0, 135.0), (281.0, 135.0), (336.0, 134.0)] {
        px(p, l, t, x, y0, x + 1.0, 183.0, gray(0x24));
    }

    // LFO display: waveform scaled by Amount, Duty, Phase / Spin.
    let disp = rr(l, t, 131.0, 21.0, 280.0, 130.0);
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));
    let rnd = [0.3, -0.7, 0.9, -0.2, 0.5, -0.9, 0.1, 0.7];
    let pts: Vec<Pos2> = (0..=274)
        .map(|i| i as f32 * 0.5)
        .map(|u| {
            let ph = (u / 137.0 + d.phase * if d.spin_mode || d.is_random() { 0.0 } else { 1.0 }).rem_euclid(1.0);
            let n = (u / 137.0 * 8.0) as usize % 8;
            let v = lfo_wave(d.wave, ph, d.duty, (rnd[n], rnd[(n + 1) % 8]));
            pos2(l + 137.0 + u, t + 59.0 - v * d.lfo_amt * 30.0)
        })
        .collect();
    p.with_clip_rect(disp.shrink(1.0)).add(Shape::line(pts, Stroke::new(2.0, if cx.on { TEAL } else { gray(0x75) })));
    let lab = gray(0x86);
    text_at(p, l + 143.0, t + 107.0, Align2::LEFT_BOTTOM, "Duty C", font(FS), lab);
    drag_c(cx, ui, p, l + 158.5, t + 122.0, shp::DUTY, &format!("{:.1} %", d.duty * 100.0), ORANGE);
    let (bl, bsel) = if d.is_random() {
        ("Width", true)
    } else if d.spin_mode {
        ("Spin", true)
    } else {
        ("Phase", false)
    };
    let br = rr(l, t, 184.0, 97.0, 229.0, 110.0);
    boxed(p, br, if bsel && cx.on { TEAL } else { gray(0x1e) });
    ct(p, br.center().x, br.top() + 10.0, bl, if bsel && cx.on { gray(0x20) } else { text() });
    if cx.toggle(ui, br, shp::SPIN, !d.is_random()) {
        set(cx, |q| q.spin_mode = !q.spin_mode);
    }
    // Phase icon: ring with a dot.
    glyph(p, l + 185.0, t + 114.0, &PHASE_ICON, lab);
    let pv = if d.spin_mode || d.is_random() { format!("{:.1} %", d.phase * 100.0) } else { format!("{:.0}°", d.phase * 360.0) };
    drag_c(cx, ui, p, l + 210.5, t + 122.0, shp::PHASE, &pv, TEAL);

    // Env Fol display.
    p.rect_filled(rr(l, t, 282.0, 21.0, 337.0, 130.0), CornerRadius::same(2), gray(0x18));
    ct(p, l + 309.5, t + 44.0, "Attack", lab);
    drag_c(cx, ui, p, l + 309.5, t + 59.0, shp::ENV_ATTACK, &ms(d.env_attack), lab);
    ct(p, l + 309.5, t + 107.0, "Release", lab);
    drag_c(cx, ui, p, l + 309.5, t + 122.0, shp::ENV_RELEASE, &ms(d.env_release), lab);

    // Mode / Output.
    ct(p, l + 367.0, t + 30.0, "Mode", text());
    for (i, m) in SH_MODES.iter().enumerate() {
        let y = 34.0 + 16.0 * i as f32;
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr(l, t, 343.0, y, 390.0, y + 13.0), shp::MODE, m, d.mode == i as u8, true) {
            set(cx, |q| q.mode = i as u8);
        }
    }
    cx.salt = 0;
    ct(p, l + 366.5, t + 94.0, "Output", text());
    if tbutton(cx, ui, p, rr(l, t, 343.0, 98.0, 390.0, 111.0), shp::DRIVE_ON, "Drive", d.drive_on, d.mode == 2) {
        set(cx, |q| q.drive_on = !q.drive_on);
    }
    let ob = rr(l, t, 343.0, 114.0, 390.0, 127.0);
    value_box_ex(cx, ui, p, ob, shp::OUTPUT, &format!("{:.1} dB", d.output), true, false, panel());

    // LFO row.
    text_at(p, l + 139.0, t + 144.0, Align2::LEFT_BOTTOM, "LFO", bold(FS), text());
    let wr = rr(l, t, 138.0, 152.0, 183.0, 165.0);
    if let Some(i) = chooser(cx, ui, p, wr, shp::WAVE, SH_WAVES[(d.wave as usize).min(9)], &SH_WAVES, true) {
        set(cx, |q| q.wave = i as u8);
    }
    if tbutton(cx, ui, p, rr(l, t, 138.0, 169.0, 159.0, 182.0), shp::LFO_SYNC, "Hz", !d.lfo_sync, true) {
        set(cx, |q| q.lfo_sync = false);
    }
    cx.salt = 1;
    if sync_btn(cx, ui, p, rr(l, t, 162.0, 169.0, 183.0, 182.0), shp::LFO_SYNC, d.lfo_sync, true) {
        set(cx, |q| q.lfo_sync = true);
    }
    cx.salt = 0;
    if d.lfo_sync {
        knob_lv(cx, ui, p, l + 208.5, t + 144.0, shp::LFO_DIV, "Rate", div(d.lfo_div), 0.0, KN, 14.5, 36.0, true);
    } else {
        knob_lv(cx, ui, p, l + 208.5, t + 144.0, shp::RATE, "Rate", &hz(d.rate), 0.0, KN, 14.5, 36.0, true);
    }
    knob_lv(cx, ui, p, l + 254.5, t + 144.0, shp::LFO_AMT, "Amount", &amt_unit(d.lfo_amt), 0.0, KN, 14.5, 36.0, true);

    // Env Fol row.
    if tbutton(cx, ui, p, rr(l, t, 288.0, 135.0, 330.0, 148.0), shp::ENV_ON, "Env Fol", d.env_on, true) {
        set(cx, |q| q.env_on = !q.env_on);
    }
    ct(p, l + 308.0, t + 165.0, "Amount", text());
    let ea = rr(l, t, 288.0, 169.0, 330.0, 182.0);
    value_box_ex(cx, ui, p, ea, shp::ENV_AMT, &amt_unit(d.env_amt), true, false, if d.env_on { box_fill(cx.on) } else { panel() });

    knob_lv(cx, ui, p, l + 367.0, t + 144.0, shp::DRY_WET, "Dry/Wet", &format!("{:.0} %", d.dry_wet * 100.0), 0.0, KN, 14.5, 36.0, true);
}

/// Left strip: fold button, vertical "MIDI Input", LED.
fn strip(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &ShifterParams) {
    px(p, l, t, 19.0, 21.0, 20.0, 184.0, gray(0x24));
    let fc = pos2(l + 9.5, t + 27.5);
    p.circle_filled(fc, 6.5, gray(0x12));
    p.circle_filled(fc, 5.5, gray(0x1c));
    let tri = if d.midi_open {
        vec![pos2(l + 7.0, t + 24.0), pos2(l + 13.5, t + 27.5), pos2(l + 7.0, t + 31.0)]
    } else {
        vec![pos2(l + 6.0, t + 25.0), pos2(l + 13.0, t + 25.0), pos2(l + 9.5, t + 31.5)]
    };
    p.add(Shape::convex_polygon(tri, gray(0xb5), Stroke::NONE));
    if cx.toggle(ui, Rect::from_center_size(fc, vec2(13.0, 13.0)), shp::OPEN, true) {
        set(cx, |q| q.midi_open = !q.midi_open);
    }
    let gal = p.layout_no_wrap("MIDI Input".into(), font(FS), text());
    let asc = gal.rows.first().map_or(8.0, |r| r.row.glyphs.first().map_or(8.0, |gl| gl.pos.y));
    p.add(egui::epaint::TextShape::new(pos2(l + 13.0 - asc, t + 171.0), gal, text()).with_angle(-std::f32::consts::FRAC_PI_2));
    let lit = d.midi_mode && d.midi.source != 0 && cx.on;
    p.circle_filled(pos2(l + 9.5, t + 178.5), 3.5, if lit { ORANGE } else { gray(0x24) });
}

/// Unfolded MIDI Input column: Internal / MIDI, MIDI From, Glide, PB.
fn midi_column(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &ShifterParams) {
    px(p, l, t, 107.0, 21.0, 108.0, 184.0, gray(0x24));
    text_at(p, l + 27.0, t + 30.0, Align2::LEFT_BOTTOM, "Mode", font(FS), text());
    if tbutton(cx, ui, p, rr(l, t, 26.0, 34.0, 70.0, 47.0), shp::MIDI_MODE, "Internal", !d.midi_mode, true) {
        set(cx, |q| q.midi_mode = false);
    }
    cx.salt = 1;
    if tbutton(cx, ui, p, rr(l, t, 73.0, 34.0, 101.0, 47.0), shp::MIDI_MODE, "MIDI", d.midi_mode, true) {
        set(cx, |q| q.midi_mode = true);
    }
    cx.salt = 0;
    text_at(p, l + 28.0, t + 65.0, Align2::LEFT_BOTTOM, "MIDI From", font(FS), text());
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
    let cr = rr(l, t, 26.0, 68.0, 101.0, 81.0);
    let shown = fit(p, &cur, &font(FS), cr.width() - 14.0);
    if let Some(i) = chooser(cx, ui, p, cr, shp::SOURCE, &shown, &items, true) {
        let src = if i == 0 { 0 } else { tracks[i - 1].0 };
        set(cx, |q| q.midi.source = src);
    }
    let ext = d.midi.source != 0;
    let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
    let tr = rr(l, t, 26.0, 84.0, 101.0, 97.0);
    if !ext {
        boxed(p, tr, box_fill(cx.on));
    } else if let Some(i) = chooser(cx, ui, p, tr, shp::TAP, d.midi.tap.label(), &taps, true) {
        set(cx, |q| q.midi.tap = ScTap::ALL[i]);
    }
    text_at(p, l + 26.0, t + 162.0, Align2::LEFT_BOTTOM, "Glide", font(FS), text());
    value_box_ex(cx, ui, p, rr(l, t, 56.0, 152.0, 101.0, 165.0), shp::GLIDE, &ms(d.glide), true, false, box_fill(cx.on));
    text_at(p, l + 27.0, t + 179.0, Align2::LEFT_BOTTOM, "PB", font(FS), text());
    value_box_ex(cx, ui, p, rr(l, t, 56.0, 169.0, 101.0, 182.0), shp::PB, &format!("{:.0} st", d.pb), true, false, box_fill(cx.on));
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Shifter(ShifterParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    if let Some(v) = norm(p, k) {
        let n = match k {
            shp::COARSE => 96.0,
            shp::PB => 24.0,
            _ => 100.0,
        };
        set_norm(p, k, ((v * n).round() + dir) / n);
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        shp::COARSE => "Pitch Coarse",
        shp::COARSE_HZ => "Freq Coarse",
        shp::FINE => "Fine",
        shp::SPREAD => "Spread",
        shp::WINDOW => "Window",
        shp::TONE => "Tone",
        shp::DELAY_MS => "Delay Time",
        shp::DELAY_DIV => "Delay Sync Rate",
        shp::FEEDBACK => "Feedback",
        shp::RATE => "LFO Rate",
        shp::LFO_DIV => "LFO Sync Rate",
        shp::LFO_AMT => "LFO Amount",
        shp::DUTY => "Duty Cycle",
        shp::PHASE => "Phase / Spin",
        shp::OFFSET => "Offset",
        shp::ENV_AMT => "Env Amount",
        shp::ENV_ATTACK => "Env Attack",
        shp::ENV_RELEASE => "Env Release",
        shp::OUTPUT => "Output",
        shp::DRY_WET => "Dry/Wet",
        shp::GLIDE => "Glide",
        shp::PB => "Pitch Bend Range",
        shp::MODE => "Mode",
        shp::WIDE => "Wide",
        shp::DELAY_ON => "Delay",
        shp::DELAY_SYNC => "Delay Sync",
        shp::WAVE => "LFO Waveform",
        shp::LFO_SYNC => "LFO Sync",
        shp::SPIN => "Phase / Spin Mode",
        shp::ENV_ON => "Env Fol",
        shp::DRIVE_ON => "Drive",
        shp::MIDI_MODE => "Pitch Source",
        shp::SOURCE => "MIDI From",
        shp::TAP => "MIDI From Tap",
        _ => "",
    }
}
