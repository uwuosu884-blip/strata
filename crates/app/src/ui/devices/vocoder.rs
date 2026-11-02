//! Vocoder (Live 12): Carrier column (chooser, Enhance, Noise XY pad /
//! External source / Pitch Tracking, Unvoiced, Sens., Fast), the band
//! display (click/drag to attenuate bands) with Bands / Range / Width /
//! Gate / Output and Precise/Retro, and the Depth / Attack / Release /
//! channel / Formant / Dry/Wet column. Geometry measured from Live 12 at
//! 100 % (l/t = device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::fx4::*;
use super::fx5::*;
use super::modfx::drag_text;
use super::*;
use daw_project::devices::dev_tap;
use daw_project::devices::params5::vcp;
use daw_project::devices::vocoder::*;

pub fn width(_d: &VocoderParams) -> f32 {
    436.0
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut VocoderParams)) {
    if let FxParams::Vocoder(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn hz(f: f32) -> String {
    if f >= 999.5 { format!("{} kHz", sig3(f / 1000.0)) } else { format!("{} Hz", sig3(f)) }
}

fn db_inf(x: f32, floor: f32) -> String {
    if x <= floor + 1e-3 { "-∞ dB".into() } else { format!("{x:.1} dB") }
}

/// Band dash x range (device coords) of band `i` of `n`.
fn band_x(i: usize, n: usize) -> (f32, f32) {
    let pitch = 204.0 / (n.max(2) - 1) as f32;
    let x0 = (111.0 + i as f32 * pitch).floor();
    (x0, (111.0 + (i + 1) as f32 * pitch).floor() - 2.0)
}

const LV_TOP: f32 = 32.0;
const LV_SPAN: f32 = 99.0;

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Vocoder(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let lab = gray(0x86);

    // ---- Carrier column ----
    text_at(p, l + 5.0, t + 32.0, Align2::LEFT_BOTTOM, "Carrier", font(FS), text());
    if tbutton(cx, ui, p, rr(l, t, 49.0, 22.0, 95.0, 35.0), vcp::ENHANCE, "Enhance", d.enhance, true) {
        set(cx, |q| q.enhance = !q.enhance);
    }
    let ci = (d.carrier as usize).min(VC_CARRIERS.len() - 1);
    if let Some(i) = chooser(cx, ui, p, rr(l, t, 5.0, 38.0, 95.0, 51.0), vcp::CARRIER, VC_CARRIERS[ci], &VC_CARRIERS, true) {
        set(cx, |q| {
            q.carrier = i as u8;
            q.ext.on = i == 1;
        });
    }
    let pad = rr(l, t, 5.0, 55.0, 95.0, 127.0);
    match ci {
        0 => {
            p.rect_filled(pad, CornerRadius::same(2), gray(0x18));
            let c = pos2(l + 11.5 + d.noise_rate * 77.0, t + 120.5 - d.noise_density * 59.0);
            p.circle_stroke(c, 5.2, Stroke::new(1.6, if cx.on { ORANGE } else { gray(0x75) }));
            xy_drag(cx, ui, pad, vcp::NOISE_RATE, vcp::NOISE_DENS, None, 77.0, 59.0);
        }
        1 => external(cx, ui, p, l, t, &d),
        3 => {
            let wi = (d.wave as usize).min(VC_WAVES.len() - 1);
            if let Some(i) = chooser(cx, ui, p, rr(l, t, 5.0, 55.0, 95.0, 68.0), vcp::WAVE, VC_WAVES[wi], &VC_WAVES, true) {
                set(cx, |q| q.wave = i as u8);
            }
            for (i, (lb, k, s)) in
                [("High", vcp::TRACK_HI, hz(d.track_hi)), ("Low", vcp::TRACK_LO, hz(d.track_lo)), ("Pitch", vcp::PITCH, st0(d.pitch))]
                    .into_iter()
                    .enumerate()
            {
                let y = 74.0 + 17.0 * i as f32;
                text_at(p, l + 6.0, t + y + 10.0, Align2::LEFT_BOTTOM, lb, font(FS), text());
                value_box(cx, ui, p, rr(l, t, 45.0, y, 95.0, y + 13.0), k, &s, true);
            }
        }
        _ => {
            p.rect_filled(pad, CornerRadius::same(2), gray(0x18));
        }
    }
    px(p, l, t, 4.0, 132.0, 96.0, 133.0, gray(0x24));
    knob_lv(cx, ui, p, l + 28.5, t + 144.0, vcp::UNVOICED, "Unvoiced", &db_inf(d.unvoiced, VC_UNVOICED.0), 0.0, KN, 15.5, 36.0, true);
    text_at(p, l + 73.5, t + 144.0, Align2::CENTER_BOTTOM, "Sens.", font(FS), text());
    value_box(cx, ui, p, rr(l, t, 55.0, 147.0, 95.0, 160.0), vcp::SENS, &format!("{:.1} %", d.sens * 100.0), true);
    let fl = if d.fast { "Fast" } else { "Slow" };
    if tbutton(cx, ui, p, rr(l, t, 55.0, 170.0, 95.0, 183.0), vcp::FAST, fl, d.fast, true) {
        set(cx, |q| q.fast = !q.fast);
    }

    // ---- Band display ----
    let disp = rr(l, t, 100.0, 21.0, 335.0, 184.0);
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));
    let tc = if cx.on { TEAL } else { gray(0x75) };
    for (x, y, sx, sy) in [(106.0, 27.0, 1.0, 1.0), (328.0, 27.0, -1.0, 1.0), (106.0, 137.0, 1.0, -1.0), (328.0, 137.0, -1.0, -1.0)] {
        let (hx0, hx1) = if sx > 0.0 { (x, x + 9.0) } else { (x - 8.0, x + 1.0) };
        let (vy0, vy1) = if sy > 0.0 { (y, y + 9.0) } else { (y - 8.0, y + 1.0) };
        px(p, l, t, hx0, y, hx1, y + 1.0, tc);
        px(p, l, t, x, vy0, x + 1.0, vy1, tc);
    }
    let n = d.n_bands();
    let tap = dev_tap(cx.dev.id);
    let mut live = false;
    for i in 0..n {
        let (x0, x1) = band_x(i, n);
        let lv = d.band_lv[i].clamp(0.0, 1.0);
        let m = if cx.on { tap.take(TAP_BANDS + i, 0.0).clamp(0.0, 1.0) } else { 0.0 };
        if m > 0.002 {
            live = true;
            let y = LV_TOP + LV_SPAN * (1.0 - m * lv);
            px(p, l, t, x0, y.round(), x1 + 1.0, LV_TOP + LV_SPAN + 1.0, gray(0x4a));
        }
        let y = (LV_TOP + LV_SPAN * (1.0 - lv)).round();
        px(p, l, t, x0, y, x1 + 1.0, y + 1.0, if cx.on { ORANGE } else { gray(0x75) });
    }
    if live {
        cx.app.animating = true;
    }
    // Click / drag on the bands sets their attenuation.
    let area = rr(l, t, 106.0, 27.0, 329.0, 138.0);
    let resp = ui.interact(area, egui::Id::unique(("vc_bands", cx.dev.id)), Sense::click_and_drag());
    if resp.double_clicked() {
        set(cx, |q| q.band_lv = [1.0; VC_MAXB]);
    } else if (resp.dragged() || resp.clicked())
        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
    {
        let x = pos.x - l;
        let lv = (1.0 - (pos.y - t - LV_TOP) / LV_SPAN).clamp(0.0, 1.0);
        if let Some(i) = (0..n).find(|&i| {
            let (x0, x1) = band_x(i, n);
            x >= x0 - 1.0 && x <= x1 + 2.0
        }) {
            set(cx, |q| q.band_lv[i] = lv);
        }
    }

    for (x, s) in [(108.0, "Bands"), (151.0, "Range"), (200.0, "Width"), (245.0, "Gate"), (289.0, "Output")] {
        text_at(p, l + x, t + 149.0, Align2::LEFT_BOTTOM, s, font(FS), lab);
    }
    // Bands: number + ▼ menu.
    let bi = (d.bands as usize).min(VC_BANDS.len() - 1);
    text_at(p, l + 109.0, t + 162.0, Align2::LEFT_BOTTOM, &VC_BANDS[bi].to_string(), font(FS), tc);
    p.add(Shape::convex_polygon(
        vec![pos2(l + 137.5, t + 157.5), pos2(l + 143.5, t + 157.5), pos2(l + 140.5, t + 161.0)],
        tc,
        Stroke::NONE,
    ));
    let br = rr(l, t, 106.0, 152.0, 146.0, 164.0);
    let resp = cx.sense(ui, br, vcp::BANDS, Sense::click());
    if resp.clicked() {
        cx.focus(vcp::BANDS);
    }
    let mut pick = None;
    menu::dropdown_aligned(&resp, 80.0, egui::RectAlign::BOTTOM_START, |ui| {
        for (i, b) in VC_BANDS.iter().enumerate() {
            if menu::check_item(ui, &b.to_string(), "", i == bi, true) {
                pick = Some(i);
            }
        }
    });
    if let Some(i) = pick {
        set(cx, |q| q.bands = i as u8);
    }
    drag_text(cx, ui, p, l + 153.0, t + 162.0, vcp::HI, &hz(d.hi), true, TEAL);
    drag_text(cx, ui, p, l + 152.0, t + 177.0, vcp::LO, &hz(d.lo), true, TEAL);
    drag_text(cx, ui, p, l + 203.0, t + 162.0, vcp::BW, &format!("{:.0} %", d.bw * 100.0), true, TEAL);
    drag_text(cx, ui, p, l + 246.0, t + 162.0, vcp::GATE, &db_inf(d.gate, VC_GATE.0), true, ORANGE);
    drag_text(cx, ui, p, l + 290.0, t + 162.0, vcp::LEVEL, &format!("{:.1} dB", d.level), true, ORANGE);
    let pr = rr(l, t, 201.0, 168.0, 239.0, 179.0);
    p.rect_filled(pr, CornerRadius::ZERO, if cx.on { ORANGE } else { gray(0x75) });
    text_at(p, pr.center().x, t + 178.0, Align2::CENTER_BOTTOM, if d.retro { "Retro" } else { "Precise" }, font(FS), gray(0x20));
    if cx.toggle(ui, pr, vcp::RETRO, true) {
        set(cx, |q| q.retro = !q.retro);
    }

    // ---- Right column ----
    knob_lv(cx, ui, p, l + 361.5, t + 32.0, vcp::DEPTH, "Depth", &format!("{:.0} %", d.depth * 100.0), 0.5, KN, 21.5, 42.0, true);
    marker(p, l + 361.5, t + 53.5, marker_col(cx, true, (d.depth - 1.0).abs() > 0.004));
    knob_lv(cx, ui, p, l + 361.5, t + 94.0, vcp::ATTACK, "Attack", &ms(d.attack), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 361.5, t + 145.0, vcp::RELEASE, "Release", &ms(d.release), 0.0, KN, 15.5, 36.0, true);
    for (i, s) in ["Mono", "Stereo", "L/R"].into_iter().enumerate() {
        let y = 22.0 + 16.0 * i as f32;
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr(l, t, 390.0, y, 429.0, y + 13.0), vcp::CHAN, s, d.chan == i as u8, true) {
            set(cx, |q| q.chan = i as u8);
        }
    }
    cx.salt = 0;
    knob_lv(cx, ui, p, l + 410.5, t + 88.0, vcp::FORMANT, "Formant", &format!("{:.2}", d.formant), 0.5, KN, 21.5, 42.0, true);
    marker(p, l + 410.5, t + 109.5, marker_col(cx, true, d.formant.abs() > 0.004));
    knob_lv(cx, ui, p, l + 410.5, t + 145.0, vcp::DRY_WET, "Dry/Wet", &format!("{:.0} %", d.dry_wet * 100.0), 0.0, KN, 14.5, 36.0, true);
}

/// External carrier: Audio From source + tap choosers in the pad area.
fn external(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &VocoderParams) {
    let own = match crate::fx_ops::top_of(&cx.app.project, cx.chain) {
        ChainRef::Track(id) => Some(id),
        _ => None,
    };
    let prj = &cx.app.project;
    let tracks: Vec<(u64, String)> =
        prj.tracks.iter().enumerate().filter(|(_, x)| Some(x.id) != own).map(|(i, x)| (x.id, prj.track_label(i))).collect();
    let cur = tracks.iter().find(|x| x.0 == d.ext.source).map_or("No Input".to_string(), |x| x.1.clone());
    let mut items: Vec<&str> = vec!["No Input"];
    items.extend(tracks.iter().map(|x| x.1.as_str()));
    text_at(p, l + 6.0, t + 66.0, Align2::LEFT_BOTTOM, "Audio From", font(FS), text());
    let cr = rr(l, t, 5.0, 70.0, 95.0, 83.0);
    let shown = fit(p, &cur, &font(FS), cr.width() - 14.0);
    if let Some(i) = chooser(cx, ui, p, cr, vcp::SOURCE, &shown, &items, true) {
        let src = if i == 0 { 0 } else { tracks[i - 1].0 };
        set(cx, |q| q.ext.source = src);
    }
    let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
    let tr = rr(l, t, 5.0, 86.0, 95.0, 99.0);
    if d.ext.source == 0 {
        boxed(p, tr, box_fill(cx.on));
    } else if let Some(i) = chooser(cx, ui, p, tr, vcp::TAP, d.ext.tap.label(), &taps, true) {
        set(cx, |q| q.ext.tap = ScTap::ALL[i]);
    }
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Vocoder(VocoderParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    if let Some(v) = norm(p, k) {
        let n = match k {
            vcp::BANDS => (VC_BANDS.len() - 1) as f32,
            vcp::PITCH => 48.0,
            _ => 100.0,
        };
        set_norm(p, k, ((v * n).round() + dir) / n);
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        vcp::UNVOICED => "Unvoiced",
        vcp::SENS => "Sensitivity",
        vcp::NOISE_RATE => "Noise Rate",
        vcp::NOISE_DENS => "Noise Crackle",
        vcp::TRACK_LO => "Lower Pitch Cutoff",
        vcp::TRACK_HI => "Upper Pitch Cutoff",
        vcp::PITCH => "Oscillator Pitch",
        vcp::BANDS => "Bands",
        vcp::LO => "Min Freq",
        vcp::HI => "Max Freq",
        vcp::BW => "Band Width",
        vcp::GATE => "Gate Threshold",
        vcp::LEVEL => "Output Level",
        vcp::DEPTH => "Depth",
        vcp::ATTACK => "Attack",
        vcp::RELEASE => "Release",
        vcp::FORMANT => "Formant Shift",
        vcp::DRY_WET => "Dry/Wet",
        vcp::CARRIER => "Carrier",
        vcp::ENHANCE => "Enhance",
        vcp::WAVE => "Oscillator Waveform",
        vcp::FAST => "Fast / Slow",
        vcp::RETRO => "Precise / Retro",
        vcp::CHAN => "Mono / Stereo",
        vcp::SOURCE => "Audio From",
        vcp::TAP => "Audio From Tap",
        _ => "",
    }
}
