//! Phaser-Flanger (Live 12): notch / delay display, Phaser / Flanger /
//! Doubler, mode parameters, LFO (Hz / sync, Freq, Amount), Feedback + Ø,
//! Output / Warmth / Dry/Wet; the unfolded section (title triangle) adds the
//! LFO shape (waveform, Duty Cycle, Phase / Spin), LFO 2 (Mix, Freq), Env
//! Fol (Amount, Attack, Release) and Safe Bass. Geometry measured from
//! Live 12 at 100 % (l/t = device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::fx4::*;
use super::fx5::*;
use super::modfx::{drag_text, text_menu};
use super::shifter::sync_btn;
use super::*;
use daw_engine::modfx::SYNC_DIVS;
use daw_engine::shifter::lfo_wave;
use daw_project::devices::params6::pfp;
use daw_project::devices::phaserflanger::*;

/// Inserted width of the unfolded section.
pub const OPEN_W: f32 = 177.0;

pub fn width(d: &PhaserFlangerParams) -> f32 {
    if d.open { 273.0 + OPEN_W } else { 273.0 }
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut PhaserFlangerParams)) {
    if let FxParams::PhaserFlanger(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn div(i: u8) -> &'static str {
    SYNC_DIVS[(i as usize).min(SYNC_DIVS.len() - 1)].1
}

fn rate_s(f: f32) -> String {
    if f >= 10.0 { format!("{f:.1} Hz") } else { format!("{f:.2} Hz") }
}

/// Grid x of `f` Hz in the notch display (48 px per decade from 10 Hz).
fn fx(f: f32) -> f32 {
    33.5 + 48.0 * (f.max(1.0) / 10.0).log10()
}

/// Title-bar triangle in a circle (centre x 25.5): ▼ folded, ▶ unfolded.
pub(super) fn title_tri(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, open: bool, k: u8) -> bool {
    let c = pos2(l + 25.5, t + 8.5);
    p.circle_filled(c, 6.5, gray(0x12));
    p.circle_filled(c, 5.5, gray(0x1e));
    let pts = if open {
        vec![pos2(c.x - 2.5, c.y - 3.5), pos2(c.x + 3.5, c.y), pos2(c.x - 2.5, c.y + 3.5)]
    } else {
        vec![pos2(c.x - 3.5, c.y - 2.5), pos2(c.x + 3.5, c.y - 2.5), pos2(c.x, c.y + 3.5)]
    };
    p.add(Shape::convex_polygon(pts, text(), Stroke::NONE));
    cx.toggle(ui, Rect::from_center_size(c, vec2(13.0, 13.0)), k, true)
}

pub(super) fn title_button(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::PhaserFlanger(d) = cx.params else { return };
    if title_tri(cx, ui, p, l, t, d.open, pfp::OPEN) {
        set(cx, |q| q.open = !q.open);
    }
}

/// Outlined mode button (orange border, filled when selected).
#[allow(clippy::too_many_arguments)]
fn mode_btn(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, label: &str, sel: bool) -> bool {
    let acc = if cx.on { ORANGE } else { gray(0x75) };
    if sel {
        p.rect_filled(r, CornerRadius::ZERO, acc);
    } else {
        p.rect_stroke(r.shrink(0.5), CornerRadius::ZERO, Stroke::new(1.0, acc), egui::StrokeKind::Middle);
    }
    text_at(p, r.center().x, r.bottom() - 2.0, Align2::CENTER_BOTTOM, label, font(FS), if sel { gray(0x20) } else { acc });
    cx.toggle(ui, r, k, true)
}

/// Grey label over an orange draggable value (display parameter rows).
#[allow(clippy::too_many_arguments)]
fn param(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, label: &str, v: &str, en: bool) {
    text_at(p, x, base, Align2::LEFT_BOTTOM, label, font(FS), if en && cx.on { gray(0x86) } else { gray(0x5a) });
    drag_text(cx, ui, p, x + 1.0, base + 15.0, k, v, en, ORANGE);
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::PhaserFlanger(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let ext = if d.open { OPEN_W } else { 0.0 };
    let disp = rr(l, t, 4.0, 21.0, 213.0 + ext, 130.0);
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));
    let tap = dev_tap(cx.dev.id);
    let m = [tap.get(TAP_MOD[0]), tap.get(TAP_MOD[1])];
    cx.app.animating = cx.on;
    let acc_l = if cx.on { ORANGE } else { gray(0x75) };
    let acc_r = if cx.on { TEAL } else { gray(0x5a) };

    // Display: frequency grid with the notches (Phaser) or the delay dots.
    let clip = p.with_clip_rect(rr(l, t, 4.0, 21.0, 213.0, 130.0));
    if d.mode == 0 {
        for dec in [10.0f32, 100.0, 1000.0, 10_000.0] {
            for k in 1..10 {
                let f = dec * k as f32;
                if f > 20_000.0 {
                    break;
                }
                let x = fx(f).round();
                px(&clip, l, t, x, 43.0, x + 1.0, 61.0, if k == 1 { gray(0x3e) } else { gray(0x26) });
            }
        }
        for (ch, (y0, y1, c)) in [(43.0, 52.0, acc_l), (52.0, 61.0, acc_r)].into_iter().enumerate() {
            let mut nf = [0.0f32; PF_NOTCH_MAX as usize];
            let n = d.notch_freqs(if cx.on { m[ch] } else { 0.0 }, &mut nf);
            for f in &nf[..n] {
                let x = fx(*f);
                if x > 4.0 && x < 212.0 {
                    clip.line_segment([pos2(l + x, t + y0), pos2(l + x, t + y1)], Stroke::new(1.0, c));
                }
            }
        }
    } else {
        // Delay time (modulated) → x.
        let amt = d.amount.clamp(0.0, 1.0);
        for (ch, (y, c)) in [(47.5, acc_l), (56.5, acc_r)].into_iter().enumerate() {
            let mm = if cx.on { m[ch] } else { 0.0 };
            let x = if d.mode == 1 {
                6.0 + 1.85 * d.fl_time * (1.0 - amt * 0.5 * (mm + 1.0) * 0.98)
            } else {
                6.0 + 1.25 * d.db_time * (1.0 + amt * mm * 0.5)
            };
            clip.circle_filled(pos2(l + x.clamp(8.0, 209.0), t + y), 3.0, c);
        }
    }
    for (i, name) in PF_MODES.iter().enumerate() {
        let x0 = 8.0 + 68.0 * i as f32;
        cx.salt = i as u32;
        if mode_btn(cx, ui, p, rr(l, t, x0, 80.0, x0 + 64.0, 93.0), pfp::MODE, name, d.mode == i as u8) {
            set(cx, |q| q.mode = i as u8);
        }
    }
    cx.salt = 0;
    match d.mode {
        0 => {
            param(cx, ui, p, l + 10.0, t + 108.0, pfp::NOTCHES, "Notches", &format!("{}", d.notches), true);
            param(cx, ui, p, l + 60.0, t + 108.0, pfp::CENTER, "Center", &hz(d.center), true);
            param(cx, ui, p, l + 111.0, t + 108.0, pfp::SPREAD, "Spread", &format!("{:.0} %", d.spread * 100.0), true);
            param(cx, ui, p, l + 162.0, t + 108.0, pfp::BLEND, "Blend", &format!("{:.2}", d.blend), true);
        }
        1 => param(cx, ui, p, l + 77.0, t + 108.0, pfp::FL_TIME, "Time", &format!("{:.1} ms", d.fl_time), true),
        _ => param(cx, ui, p, l + 144.0, t + 108.0, pfp::DB_TIME, "Time", &format!("{:.1} ms", d.db_time), true),
    }

    // LFO row: Hz / sync, Freq, Amount, Feedback, Ø.
    if tbutton(cx, ui, p, rr(l, t, 8.0, 145.0, 27.0, 158.0), pfp::LFO_SYNC, "Hz", !d.lfo_sync, true) {
        set(cx, |q| q.lfo_sync = false);
    }
    cx.salt = 1;
    if sync_btn(cx, ui, p, rr(l, t, 8.0, 161.0, 27.0, 174.0), pfp::LFO_SYNC, d.lfo_sync, true) {
        set(cx, |q| q.lfo_sync = true);
    }
    cx.salt = 0;
    let line = gray(0x4a);
    px(p, l, t, 31.0, 159.0, 37.0, 160.0, line);
    px(p, l, t, 179.0, 159.0, 185.0, 160.0, line);
    if d.lfo_sync {
        knob_lv(cx, ui, p, l + 56.0, t + 144.0, pfp::DIV, "Freq", div(d.div), 0.0, KN, 14.5, 36.0, true);
    } else {
        knob_lv(cx, ui, p, l + 56.0, t + 144.0, pfp::RATE, "Freq", &rate_s(d.rate), 0.0, KN, 14.5, 36.0, true);
    }
    knob_lv(cx, ui, p, l + 108.0, t + 144.0, pfp::AMOUNT, "Amount", &format!("{:.0} %", d.amount * 100.0), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 160.0, t + 144.0, pfp::FEEDBACK, "Feedback", &format!("{:.1} %", d.feedback * 100.0), 0.0, KN, 14.5, 36.0, true);
    let inv = rr(l, t, 190.0, 152.0, 205.0, 166.0);
    boxed(p, inv, if d.fb_invert && cx.on { ORANGE } else { box_fill(cx.on) });
    text_at(
        p,
        inv.center().x,
        inv.bottom() - 2.5,
        Align2::CENTER_BOTTOM,
        "Ø",
        font(FS),
        if d.fb_invert && cx.on { gray(0x20) } else { text() },
    );
    if cx.toggle(ui, inv, pfp::FB_INV, true) {
        set(cx, |q| q.fb_invert = !q.fb_invert);
    }

    if d.open {
        open_section(cx, ui, p, l, t, &d);
        px(p, l, t, 213.0, 21.0, 214.0, 130.0, gray(0x3e));
        px(p, l, t, 329.0, 25.0, 330.0, 126.0, gray(0x3e));
        for x in [213.0, 329.0, 389.0] {
            px(p, l, t, x, 135.0, x + 1.0, 184.0, gray(0x24));
        }
    } else {
        px(p, l, t, 213.0, 135.0, 214.0, 184.0, gray(0x24));
    }

    // Right column.
    let rx = l + 243.0 + ext;
    knob_lv(cx, ui, p, rx, t + 31.0, pfp::OUTPUT, "Output", &format!("{:.1} dB", d.output), 0.0, KN, 14.5, 35.5, true);
    knob_lv(cx, ui, p, rx, t + 88.0, pfp::WARMTH, "Warmth", &format!("{:.1} %", d.warmth * 100.0), 0.0, KN, 14.5, 35.5, true);
    knob_lv(cx, ui, p, rx, t + 144.0, pfp::DRY_WET, "Dry/Wet", &format!("{:.0} %", d.dry_wet * 100.0), 0.0, KN, 14.5, 36.0, true);
}

/// Unfolded section: LFO shape column, LFO 2 column, Env Fol, Safe Bass.
fn open_section(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &PhaserFlangerParams) {
    // Waveform (two cycles) with Duty.
    let wr = rr(l, t, 220.0, 38.0, 323.0, 64.0);
    let col = if cx.on { TEAL } else { gray(0x75) };
    let rnd = [0.3, -0.7, 0.9, -0.2];
    let pts: Vec<Pos2> = (0..=102)
        .map(|i| i as f32)
        .map(|u| {
            let ph = (u / 51.0).rem_euclid(1.0);
            let n = (u / 25.5) as usize % 4;
            let v = lfo_wave(d.wave, ph, d.duty, (rnd[n], rnd[(n + 1) % 4]));
            pos2(wr.left() + u, wr.center().y - v * 11.0)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.5, col)));
    let waves: Vec<&str> = PF_WAVES.to_vec();
    if let Some(i) = text_menu(
        cx,
        ui,
        p,
        rr(l, t, 220.0, 80.0, 262.0, 93.0),
        pfp::WAVE,
        l + 222.0,
        t + 92.0,
        Some(l + 256.0),
        d.wave as usize,
        &waves,
        true,
        ORANGE,
    ) {
        set(cx, |q| q.wave = i as u8);
    }
    // Phase (Φ) / Spin buttons.
    let pr = rr(l, t, 274.0, 80.0, 298.0, 93.0);
    let sr = rr(l, t, 300.0, 80.0, 323.0, 93.0);
    let acc = if cx.on { ORANGE } else { gray(0x75) };
    for (i, (rct, sel)) in [(pr, !d.spin_mode), (sr, d.spin_mode)].into_iter().enumerate() {
        if sel {
            p.rect_filled(rct, CornerRadius::ZERO, acc);
        } else {
            p.rect_stroke(rct.shrink(0.5), CornerRadius::ZERO, Stroke::new(1.0, acc), egui::StrokeKind::Middle);
        }
        let ic = if sel { gray(0x20) } else { acc };
        let c = rct.center();
        if i == 0 {
            p.circle_stroke(c, 3.5, Stroke::new(1.2, ic));
            p.line_segment([pos2(c.x, c.y - 5.5), pos2(c.x, c.y + 5.5)], Stroke::new(1.2, ic));
        } else {
            p.circle_stroke(c, 4.5, Stroke::new(1.2, ic));
            p.circle_filled(pos2(c.x - 1.5, c.y), 1.8, ic);
        }
        cx.salt = i as u32;
        if cx.toggle(ui, rct, pfp::SPIN_MODE, true) && !sel {
            set(cx, |q| q.spin_mode = i == 1);
        }
    }
    cx.salt = 0;
    param(cx, ui, p, l + 220.0, t + 108.0, pfp::DUTY, "Duty Cyc", &format!("{:.0} %", d.duty * 100.0), true);
    if d.spin_mode {
        param(cx, ui, p, l + 274.0, t + 108.0, pfp::SPIN, "Spin", &format!("{:.1} %", d.spin * 100.0), true);
    } else {
        param(cx, ui, p, l + 274.0, t + 108.0, pfp::PHASE, "Phase", &format!("{:.1}°", d.phase * 360.0), true);
    }

    // LFO 2.
    param(cx, ui, p, l + 336.0, t + 36.0, pfp::LFO2_MIX, "LFO2 Mix", &format!("{:.1} %", d.lfo2_mix * 100.0), true);
    let hzr = rr(l, t, 336.0, 80.0, 360.0, 93.0);
    let syr = rr(l, t, 362.0, 80.0, 385.0, 93.0);
    for (i, (rct, sel)) in [(hzr, !d.lfo2_sync), (syr, d.lfo2_sync)].into_iter().enumerate() {
        if sel {
            p.rect_filled(rct, CornerRadius::ZERO, acc);
        } else {
            p.rect_stroke(rct.shrink(0.5), CornerRadius::ZERO, Stroke::new(1.0, acc), egui::StrokeKind::Middle);
        }
        let ic = if sel { gray(0x20) } else { acc };
        if i == 0 {
            text_at(p, rct.center().x, rct.bottom() - 2.0, Align2::CENTER_BOTTOM, "Hz", font(FS), ic);
        } else {
            glyph(p, rct.center().x - 4.5, rct.top() + 2.0, &METRO, ic);
        }
        cx.salt = 2 + i as u32;
        if cx.toggle(ui, rct, pfp::LFO2_SYNC, true) && !sel {
            set(cx, |q| q.lfo2_sync = i == 1);
        }
    }
    cx.salt = 0;
    if d.lfo2_sync {
        param(cx, ui, p, l + 336.0, t + 108.0, pfp::LFO2_DIV, "LFO2 Freq", div(d.lfo2_div), true);
    } else {
        param(cx, ui, p, l + 336.0, t + 108.0, pfp::LFO2_RATE, "LFO2 Freq", &rate_s(d.lfo2_rate), true);
    }

    // Env Fol.
    if tbutton(cx, ui, p, rr(l, t, 220.0, 138.0, 269.0, 151.0), pfp::ENV_ON, "Env Fol", d.env_on, true) {
        set(cx, |q| q.env_on = !q.env_on);
    }
    let ea = rr(l, t, 274.0, 138.0, 323.0, 151.0);
    value_box_ex(cx, ui, p, ea, pfp::ENV_AMT, &format!("{:.2}", d.env_amt), d.env_on, false, box_fill(cx.on));
    // Bipolar marker at the right of the amount box.
    p.add(Shape::convex_polygon(
        vec![pos2(ea.right() - 2.0, ea.top() + 3.0), pos2(ea.right() - 2.0, ea.bottom() - 3.0), pos2(ea.right() - 7.0, ea.center().y)],
        gray(0x2e),
        Stroke::NONE,
    ));
    text_at(p, l + 221.0, t + 164.0, Align2::LEFT_BOTTOM, "Attack", font(FS), text());
    value_box(cx, ui, p, rr(l, t, 220.0, 166.0, 269.0, 179.0), pfp::ENV_ATTACK, &ms(d.env_attack), d.env_on);
    text_at(p, l + 275.0, t + 164.0, Align2::LEFT_BOTTOM, "Release", font(FS), text());
    value_box(cx, ui, p, rr(l, t, 274.0, 166.0, 323.0, 179.0), pfp::ENV_RELEASE, &format!("{} ms", sig3(d.env_release)), d.env_on);

    // Safe Bass.
    knob_lv(cx, ui, p, l + 361.0, t + 144.0, pfp::SAFE_BASS, "Safe Bass", &hz(d.safe_bass), 0.0, KN, 14.5, 36.0, true);
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::PhaserFlanger(PhaserFlangerParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    if let Some(v) = norm(p, k) {
        let n = match k {
            pfp::NOTCHES => (PF_NOTCH_MAX - 1) as f32,
            pfp::DIV | pfp::LFO2_DIV => (SYNC_DIVS.len() - 1) as f32,
            _ => 100.0,
        };
        set_norm(p, k, ((v * n).round() + dir) / n);
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        pfp::NOTCHES => "Notches",
        pfp::CENTER => "Center Freq",
        pfp::SPREAD => "Spread",
        pfp::BLEND => "Mod Blend",
        pfp::FL_TIME => "Flange Time",
        pfp::DB_TIME => "Doubler Time",
        pfp::RATE => "LFO Rate",
        pfp::DIV => "LFO Sync Rate",
        pfp::PHASE => "LFO Phase",
        pfp::SPIN => "LFO Spin",
        pfp::OFFSET => "LFO Offset",
        pfp::DUTY => "Duty Cycle",
        pfp::LFO2_MIX => "LFO 2 Mix",
        pfp::LFO2_RATE => "LFO 2 Rate",
        pfp::LFO2_DIV => "LFO 2 Sync Rate",
        pfp::AMOUNT => "Mod Amount",
        pfp::FEEDBACK => "Feedback",
        pfp::ENV_AMT => "Env Amount",
        pfp::ENV_ATTACK => "Env Attack",
        pfp::ENV_RELEASE => "Env Release",
        pfp::SAFE_BASS => "Safe Bass",
        pfp::OUTPUT => "Output",
        pfp::WARMTH => "Warmth",
        pfp::DRY_WET => "Dry/Wet",
        pfp::MODE => "Mode",
        pfp::LFO_SYNC => "LFO Sync",
        pfp::WAVE => "LFO Waveform",
        pfp::SPIN_MODE => "LFO Stereo Mode",
        pfp::LFO2_SYNC => "LFO 2 Sync",
        pfp::FB_INV => "Feedback Invert",
        pfp::ENV_ON => "Env Fol",
        _ => "",
    }
}
