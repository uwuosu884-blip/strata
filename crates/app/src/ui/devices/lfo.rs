//! LFO (Live 12 Max for Live device): waveform scope, Map / Multimap,
//! Mod / Remote, rate, depth, offset, phase, hold, retrigger.

use super::dynamics::{KN, knob_lv, sig3};
use super::*;
use daw_engine::lfo::{LFO_MAPS, LFO_STEPS_MAX, LFO_SYNC};
use daw_project::devices::{LfoMap, LfoParams, LfoWave, dev_tap};

pub const WIDTH: f32 = 257.0;

pub(super) const LABEL: Color32 = Color32::from_rgb(0x6c, 0x6c, 0x6c);

pub(super) fn pct(x: f32) -> String {
    format!("{:.0} %", x * 100.0)
}

pub(super) fn lfo_of(p: &mut FxParams) -> Option<&mut LfoParams> {
    if let FxParams::Lfo(l) = p { Some(l) } else { None }
}

/// "Device: Parameter" of a mapping target.
fn target_name(app: &DawApp, m: &LfoMap) -> Option<String> {
    let (_, d) = app.locate_device(m.device)?;
    Some(format!("{}: {}", d.params.name(), param_name(&d.params, m.param)))
}

/// One map row: Map / unmap, Mod|Remote, ± and amount (or Min / Max).
pub(super) fn map_row(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, y: f32, i: usize) {
    let FxParams::Lfo(l0) = cx.params else { return };
    let m = l0.maps[i];
    let armed = cx.app.lfo_arm == Some((cx.dev.id, i as u8));
    let mapped = m.device != 0;
    let mr = Rect::from_min_max(pos2(l + 8.0, y), pos2(l + 76.0, y + 13.0));
    let name = if mapped { target_name(cx.app, &m) } else { None };
    // Unmapped: orange outline; mapped: orange with the parameter name;
    // armed (waiting for a click on a parameter): half-lit.
    match (&name, armed) {
        (_, true) => {
            p.rect_filled(mr, CornerRadius::ZERO, mix(ORANGE, gray(0x18), 0.45));
            text_at(p, mr.center().x, y + 10.0, Align2::CENTER_BOTTOM, "Map", font(FS), gray(0x20));
        }
        (Some(n), false) => {
            p.rect_filled(mr, CornerRadius::ZERO, ORANGE);
            text_at(p, mr.center().x, y + 10.0, Align2::CENTER_BOTTOM, &fit(p, n, &font(FS), mr.width() - 6.0), font(FS), gray(0x20));
        }
        (None, false) => {
            p.rect_stroke(mr, CornerRadius::ZERO, Stroke::new(1.0, ORANGE), egui::StrokeKind::Inside);
            text_at(p, mr.center().x, y + 10.0, Align2::CENTER_BOTTOM, "Map", font(FS), ORANGE);
        }
    }
    let resp = ui.interact(mr, egui::Id::unique(("lfo_map", cx.dev.id, i)), Sense::click());
    if resp.clicked() {
        cx.app.lfo_arm = if armed { None } else { Some((cx.dev.id, i as u8)) };
    }
    if let Some(n) = &name {
        resp.on_hover_text(n);
        // Unmap ×.
        let c = pos2(l + 85.5, y + 6.5);
        let ur = Rect::from_center_size(c, vec2(13.0, 13.0));
        p.circle_stroke(c, 5.5, Stroke::new(1.0, ORANGE));
        let s = Stroke::new(1.2, ORANGE);
        p.line_segment([c + vec2(-2.2, -2.2), c + vec2(2.2, 2.2)], s);
        p.line_segment([c + vec2(-2.2, 2.2), c + vec2(2.2, -2.2)], s);
        if cx.toggle(ui, ur, fp::map(i, fp::M_UNMAP), true)
            && let Some(l) = lfo_of(&mut cx.params)
        {
            l.maps[i] = LfoMap::default();
        }
    }
    // Mod / Remote.
    let tr = Rect::from_min_max(pos2(l + 95.0, y), pos2(l + 138.0, y + 13.0));
    p.rect_filled(tr, CornerRadius::ZERO, if m.remote { ORANGE } else { TEAL });
    text_at(p, tr.center().x, y + 10.0, Align2::CENTER_BOTTOM, if m.remote { "Remote" } else { "Mod" }, font(FS), gray(0x20));
    if cx.toggle(ui, tr, fp::map(i, fp::M_REMOTE), true)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.maps[i].remote = !l.maps[i].remote;
    }
    if m.remote {
        for (j, (f, v)) in [(fp::M_MIN, m.min), (fp::M_MAX, m.max)].into_iter().enumerate() {
            let x = l + 187.5 + 37.5 * j as f32;
            let r = Rect::from_min_max(pos2(x - 34.0, y), pos2(x + 1.0, y + 13.0));
            cx.drag(ui, r, fp::map(i, f), true);
            text_at(p, x, y + 11.0, Align2::RIGHT_BOTTOM, &pct(v), font(FS), ORANGE);
        }
    } else {
        let pr = Rect::from_min_max(pos2(l + 140.0, y), pos2(l + 154.0, y + 13.0));
        p.rect_stroke(pr, CornerRadius::ZERO, Stroke::new(1.0, TEAL), egui::StrokeKind::Inside);
        let c = pr.center();
        let s = Stroke::new(1.0, TEAL);
        p.line_segment([c + vec2(-3.5, -1.5), c + vec2(3.5, -1.5)], s);
        p.line_segment([c + vec2(0.0, -5.0), c + vec2(0.0, 2.0)], s);
        if !m.unipolar {
            p.line_segment([c + vec2(-3.5, 4.0), c + vec2(3.5, 4.0)], s);
        }
        if cx.toggle(ui, pr, fp::map(i, fp::M_POLARITY), true)
            && let Some(l) = lfo_of(&mut cx.params)
        {
            l.maps[i].unipolar = !l.maps[i].unipolar;
        }
        let ar = Rect::from_min_max(pos2(l + 156.0, y), pos2(l + 189.0, y + 13.0));
        cx.drag(ui, ar, fp::map(i, fp::M_AMOUNT), true);
        text_at(p, l + 187.5, y + 11.0, Align2::RIGHT_BOTTOM, &pct(m.amount), font(FS), TEAL);
    }
}

/// Multimap toggle (☰ in an orange box) or the "−" that closes it.
/// `x`: the button's left edge.
pub(super) fn multimap_button(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, t: f32, open: bool) {
    let r = Rect::from_min_max(pos2(x, t + 25.0), pos2(x + 14.0, t + 39.0));
    if open {
        p.rect_filled(r, CornerRadius::ZERO, ORANGE);
        quad(p, Rect::from_min_max(pos2(r.left() + 3.0, t + 31.0), pos2(r.right() - 3.0, t + 32.0)), gray(0x20));
    } else {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, ORANGE), egui::StrokeKind::Inside);
        for y in [28.0, 30.0, 32.0, 34.0] {
            quad(p, Rect::from_min_max(pos2(r.left() + 3.0, t + y), pos2(r.right() - 3.0, t + y + 1.0)), ORANGE);
        }
    }
    if cx.toggle(ui, r, fp::MULTIMAP, true)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.multimap = !l.multimap;
    }
}

pub(super) fn lfo(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Lfo(l0) = cx.params else { return };
    if l0.multimap {
        multimap_list(cx, ui, p, l, t, l + 253.0, l + 236.0, 177.0);
        return;
    }
    // Display: header row, scope, parameter row.
    let d = Rect::from_min_max(pos2(l + 4.0, t + 21.0), pos2(l + 253.0, t + 132.0));
    p.rect_filled(d, CornerRadius::same(2), gray(0x18));
    map_row(cx, ui, p, l, t + 25.0, 0);
    multimap_button(cx, ui, p, l + 236.0, t, false);
    let cy = t + 70.5;
    let amp = 26.5;
    let line_y = (cy - l0.offset * amp).round() + 0.5;
    quad(p, Rect::from_min_max(pos2(d.left(), line_y - 0.5), pos2(d.right(), line_y + 0.5)), gray(0x3e));
    // Scope: the output history over three cycles (newest at the right),
    // or the shape itself while the engine is not running it.
    let tap = dev_tap(cx.dev.id);
    let w = d.width();
    let mut pts = Vec::with_capacity(w as usize + 1);
    let hz = (l0.hz(cx.app.project.tempo_bpm) as f32).max(1e-3);
    let frames = ((tap.rate() * 3.0 / hz) as usize).clamp(2, daw_engine::devtap::RING / 2);
    let h = if tap.written() > 0 && tap.rate() > 0.0 { tap.history(frames) } else { Vec::new() };
    if h.len() >= 2 {
        // Half-pixel steps, linearly interpolated between frames (smooth,
        // anti-aliased curve instead of nearest-frame jitter).
        let miss = (frames - h.len()) as f32;
        let n = (w * 2.0) as usize;
        for x in 0..=n {
            let fi = x as f32 / n as f32 * (frames - 1) as f32 - miss;
            if fi < 0.0 {
                continue;
            }
            let i0 = (fi.floor() as usize).min(h.len() - 1);
            let i1 = (i0 + 1).min(h.len() - 1);
            let v = h[i0][0] + (h[i1][0] - h[i0][0]) * (fi - i0 as f32);
            pts.push(pos2(d.left() + x as f32 * 0.5, cy - v * amp));
        }
        cx.app.animating = true;
    } else {
        for x in 0..=w as usize {
            let ph = l0.phase as f64 + 3.0 * x as f64 / w as f64;
            let v = match l0.wave {
                LfoWave::Random | LfoWave::Bin | LfoWave::Stray | LfoWave::Glider => 0.0,
                _ => {
                    let steps = if l0.wave.steppable() && l0.steps >= 2 { l0.steps as f64 } else { 0.0 };
                    let f = ph.rem_euclid(1.0);
                    let q = if steps > 0.0 { (f * steps).floor() / steps } else { f };
                    l0.shape_at(ph.floor() + q)
                }
            };
            let out = (v * l0.depth + l0.offset).clamp(-1.0, 1.0);
            pts.push(pos2(d.left() + x as f32, cy - out * amp));
        }
    }
    if pts.len() > 1 {
        p.with_clip_rect(d.shrink(1.0)).add(Shape::line(pts, Stroke::new(1.5, if cx.on { ORANGE } else { gray(0x75) })));
    }
    // Parameter row.
    let grey_val = mix(ORANGE, gray(0x18), 0.55);
    let cols = [9.0, 70.0, 116.0, 162.0, 208.0];
    for (x, s) in cols.iter().zip(["Waveform", "Shape", "Steps", "Jitter", "Smooth"]) {
        text_at(p, l + x, t + 112.0, Align2::LEFT_BOTTOM, s, font(FS), LABEL);
    }
    // Waveform chooser (click: list).
    let wr = Rect::from_min_max(pos2(l + 7.0, t + 115.0), pos2(l + 58.0, t + 129.0));
    text_at(p, l + 9.0, t + 126.0, Align2::LEFT_BOTTOM, l0.wave.label(), font(FS), ORANGE);
    p.add(Shape::convex_polygon(
        vec![pos2(l + 50.0, t + 120.0), pos2(l + 56.0, t + 120.0), pos2(l + 53.0, t + 124.0)],
        ORANGE,
        Stroke::NONE,
    ));
    let resp = cx.sense(ui, wr, fp::WAVE, Sense::click());
    if resp.clicked() {
        cx.focus(fp::WAVE);
    }
    let mut pick = None;
    menu::dropdown_aligned(&resp, 90.0, egui::RectAlign::BOTTOM_START, |ui| {
        for (i, wv) in LfoWave::ALL.iter().enumerate() {
            if menu::check_item(ui, wv.label(), "", *wv == l0.wave, true) {
                pick = Some(i);
            }
        }
    });
    if let Some(i) = pick {
        cx.undo_step();
        if let Some(l) = lfo_of(&mut cx.params) {
            l.wave = LfoWave::ALL[i];
        }
    }
    let steps = if l0.steps < 2 { "Off".to_string() } else { l0.steps.to_string() };
    let vals = [
        (fp::SHAPE, format!("{:.0} %", l0.shape * 100.0), l0.wave.shapeable()),
        (fp::STEPS, steps, l0.wave.steppable()),
        (fp::JITTER, pct(l0.jitter), true),
        (fp::SMOOTH, pct(l0.smooth), true),
    ];
    for (x, (k, s, en)) in cols[1..].iter().zip(vals) {
        let r = Rect::from_min_max(pos2(l + x - 2.0, t + 115.0), pos2(l + x + 40.0, t + 129.0));
        cx.drag(ui, r, k, en);
        text_at(p, l + x, t + 126.0, Align2::LEFT_BOTTOM, &s, font(FS), if en { ORANGE } else { grey_val });
    }
    // Rate with time mode and ×10.
    let rate = if l0.sync {
        LFO_SYNC[(l0.sync_rate as usize).min(LFO_SYNC.len() - 1)].1.to_string()
    } else {
        let hz = l0.rate * if l0.x10 { 10.0 } else { 1.0 };
        format!("{} Hz", sig3(hz))
    };
    let rk = if l0.sync { fp::SYNC_RATE } else { fp::RATE };
    knob_lv(cx, ui, p, l + 33.5, t + 144.0, rk, "Rate", &rate, 0.0, KN, 15.5, 37.0, true);
    quad(p, Rect::from_min_max(pos2(l + 50.0, t + 153.0), pos2(l + 58.0, t + 154.0)), gray(0x55));
    let hzr = Rect::from_min_max(pos2(l + 62.0, t + 139.0), pos2(l + 82.0, t + 153.0));
    toggle_look(p, hzr, "Hz", !l0.sync, true, cx.on);
    if cx.toggle(ui, hzr, fp::SYNC, true)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.sync = false;
    }
    let nr = Rect::from_min_max(pos2(l + 62.0, t + 155.0), pos2(l + 82.0, t + 169.0));
    toggle_look(p, nr, "", l0.sync, true, cx.on);
    metronome(p, nr.center(), if l0.sync { gray(0x20) } else { text() });
    cx.salt = 1;
    let hit = cx.toggle(ui, nr, fp::SYNC, true);
    cx.salt = 0;
    if hit && let Some(l) = lfo_of(&mut cx.params) {
        l.sync = true;
    }
    let xr = Rect::from_min_max(pos2(l + 62.0, t + 173.0), pos2(l + 72.0, t + 183.0));
    boxed(p, xr, if l0.x10 { ORANGE } else { gray(0x1e) });
    text_at(p, l + 74.0, t + 182.0, Align2::LEFT_BOTTOM, "×10", font(FS), if l0.sync { dim() } else { text() });
    if cx.toggle(ui, Rect::from_min_max(xr.min, pos2(l + 92.0, xr.bottom())), fp::X10, !l0.sync)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.x10 = !l.x10;
    }
    knob_lv(cx, ui, p, l + 121.5, t + 144.0, fp::DEPTH, "Depth", &pct(l0.depth), 0.0, KN, 15.5, 37.0, true);
    knob_lv(cx, ui, p, l + 167.0, t + 144.0, fp::OFFSET, "Offset", &format!("{:.0} %", l0.offset * 100.0), 0.5, KN, 15.5, 37.0, true);
    // Phase / Hold / Retrigger.
    text_at(p, l + 222.5, t + 147.0, Align2::CENTER_BOTTOM, "Phase", font(FS), text());
    let pr = Rect::from_min_max(pos2(l + 201.0, t + 150.0), pos2(l + 245.0, t + 164.0));
    p.rect_filled(pr, CornerRadius::ZERO, gray(0x24));
    let w = (l0.phase.clamp(0.0, 1.0) * pr.width()).round();
    if w > 0.0 {
        quad(p, Rect::from_min_size(pr.min, vec2(w, pr.height())), mix(TEAL, gray(0x24), 0.4));
    }
    p.add(Shape::convex_polygon(
        vec![pos2(pr.right() - 2.0, pr.top() + 2.5), pos2(pr.right() - 2.0, pr.bottom() - 2.5), pos2(pr.right() - 7.5, pr.center().y)],
        gray(0x30),
        Stroke::NONE,
    ));
    cx.drag(ui, pr, fp::PHASE, true);
    text_at(p, l + 220.0, t + 161.0, Align2::CENTER_BOTTOM, &pct(l0.phase), font(FS), text());
    let hr = Rect::from_min_max(pos2(l + 201.0, t + 166.0), pos2(l + 229.0, t + 180.0));
    toggle_look(p, hr, "Hold", l0.hold, true, cx.on);
    if cx.toggle(ui, hr, fp::HOLD, true)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.hold = !l.hold;
    }
    let rc = pos2(l + 237.5, t + 172.5);
    p.circle_filled(rc, 6.5, gray(0x1e));
    text_at(p, rc.x, rc.y + 4.0, Align2::CENTER_BOTTOM, "R", font(FS), text());
    if cx.toggle(ui, Rect::from_center_size(rc, vec2(13.0, 13.0)), fp::RETRIG, true)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.retrig = l.retrig.wrapping_add(1);
    }
}

/// Multimap view: 8 map rows in a display ending at `right`, the close
/// button at `btn`, column labels on baseline `t + lb`.
#[allow(clippy::too_many_arguments)]
pub(super) fn multimap_list(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, right: f32, btn: f32, lb: f32) {
    let d = Rect::from_min_max(pos2(l + 4.0, t + 21.0), pos2(right, t + 183.0));
    p.rect_filled(d, CornerRadius::same(2), gray(0x18));
    for i in 0..LFO_MAPS {
        map_row(cx, ui, p, l, t + 25.0 + 16.0 * i as f32, i);
    }
    multimap_button(cx, ui, p, btn, t, true);
    text_at(p, l + 42.0, t + lb, Align2::CENTER_BOTTOM, "Parameter", font(FS), LABEL);
    text_at(p, l + 116.0, t + lb, Align2::CENTER_BOTTOM, "Mode", font(FS), LABEL);
    text_at(p, l + 193.0, t + lb, Align2::CENTER_BOTTOM, "Range", font(FS), LABEL);
    for (a, b) in [(161.0, 173.0), (213.0, 225.0)] {
        quad(p, Rect::from_min_max(pos2(l + a, t + lb - 3.0), pos2(l + b, t + lb - 2.0)), gray(0x3e));
    }
}

/// Output scope in `d` around `cy` (±`amp`): the engine's history over
/// three cycles, or `still(phase)` (−1..1, before depth / offset) while
/// the engine is not running the device.
/// `secs`: time window shown.
#[allow(clippy::too_many_arguments)]
pub(super) fn scope(cx: &mut Ctx, p: &Painter, d: Rect, cy: f32, amp: f32, secs: f32, l0: &LfoParams, still: impl Fn(f64) -> f32) {
    let tap = dev_tap(cx.dev.id);
    let w = d.width();
    let mut pts = Vec::with_capacity(w as usize * 2 + 1);
    let hz = (l0.hz(cx.app.project.tempo_bpm) as f32).max(1e-3);
    let frames = ((tap.rate() * secs) as usize).clamp(2, daw_engine::devtap::RING / 2);
    let h = if tap.written() > 0 && tap.rate() > 0.0 { tap.history(frames) } else { Vec::new() };
    if h.len() >= 2 {
        let miss = (frames - h.len()) as f32;
        let n = (w * 2.0) as usize;
        for x in 0..=n {
            let fi = x as f32 / n as f32 * (frames - 1) as f32 - miss;
            if fi < 0.0 {
                continue;
            }
            let i0 = (fi.floor() as usize).min(h.len() - 1);
            let i1 = (i0 + 1).min(h.len() - 1);
            let v = h[i0][0] + (h[i1][0] - h[i0][0]) * (fi - i0 as f32);
            pts.push(pos2(d.left() + x as f32 * 0.5, cy - v * amp));
        }
        cx.app.animating = true;
    } else {
        let n = (w * 2.0) as usize;
        for x in 0..=n {
            let ph = l0.phase as f64 + (secs * hz) as f64 * x as f64 / n as f64;
            let out = (still(ph) * l0.depth + l0.offset).clamp(-1.0, 1.0);
            pts.push(pos2(d.left() + x as f32 * 0.5, cy - out * amp));
        }
    }
    if pts.len() > 1 {
        p.with_clip_rect(d.shrink(1.0)).add(Shape::line(pts, Stroke::new(1.5, if cx.on { ORANGE } else { gray(0x75) })));
    }
}

/// Metronome glyph of the synced time mode button.
pub(super) fn metronome(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.0, col);
    let (b, top) = (c.y + 4.5, c.y - 4.5);
    p.add(Shape::closed_line(vec![pos2(c.x - 4.0, b), pos2(c.x - 1.5, top), pos2(c.x + 1.5, top), pos2(c.x + 4.0, b)], s));
    p.line_segment([pos2(c.x, b - 1.0), pos2(c.x + 4.5, top + 1.0)], s);
}

pub(super) fn name(k: u8) -> &'static str {
    if let Some((_, f)) = fp::split(k) {
        return match f {
            fp::M_AMOUNT => "Mod Amount",
            fp::M_MIN => "Min",
            fp::M_MAX => "Max",
            fp::M_REMOTE => "Mod Mode",
            fp::M_POLARITY => "Polarity",
            fp::M_MAP => "Map",
            _ => "Unmap",
        };
    }
    match k {
        fp::WAVE => "Waveform",
        fp::SHAPE => "Shape",
        fp::STEPS => "Steps",
        fp::JITTER => "Jitter",
        fp::SMOOTH => "Smooth",
        fp::RATE => "Rate",
        fp::SYNC_RATE => "Synced Rate",
        fp::SYNC => "Time Mode",
        fp::X10 => "Rate x10",
        fp::DEPTH => "Depth",
        fp::OFFSET => "Offset",
        fp::PHASE => "Phase",
        fp::HOLD => "Hold",
        fp::RETRIG => "Retrigger",
        fp::GRID => "Grid",
        fp::SNAP => "Snap",
        fp::TRIG => "Mode",
        fp::POINTS => "Shape",
        fp::PRESET => "Preset",
        _ => "Multimap",
    }
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let FxParams::Lfo(l) = p else { return };
    let d = LfoParams::default();
    if let Some((i, f)) = fp::split(k) {
        let dm = LfoMap::default();
        let m = &mut l.maps[i];
        match f {
            fp::M_AMOUNT => m.amount = dm.amount,
            fp::M_MIN => m.min = dm.min,
            fp::M_MAX => m.max = dm.max,
            _ => {}
        }
        return;
    }
    match k {
        fp::WAVE => l.wave = d.wave,
        fp::SHAPE => l.shape = d.shape,
        fp::STEPS => l.steps = d.steps,
        fp::JITTER => l.jitter = d.jitter,
        fp::SMOOTH => l.smooth = d.smooth,
        fp::RATE => l.rate = d.rate,
        fp::SYNC_RATE => l.sync_rate = d.sync_rate,
        fp::DEPTH => l.depth = d.depth,
        fp::OFFSET => l.offset = d.offset,
        fp::PHASE => l.phase = d.phase,
        fp::GRID => l.grid = d.grid,
        _ => {}
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Lfo(l) = p else { return };
    match k {
        fp::WAVE => {
            let i = LfoWave::ALL.iter().position(|&w| w == l.wave).unwrap_or(0) as i32 - dir as i32;
            l.wave = LfoWave::ALL[i.clamp(0, LfoWave::ALL.len() as i32 - 1) as usize];
        }
        fp::STEPS => l.steps = (l.steps as i32 + dir as i32).clamp(0, LFO_STEPS_MAX as i32) as u8,
        fp::SYNC_RATE => l.sync_rate = (l.sync_rate as i32 + dir as i32).clamp(0, LFO_SYNC.len() as i32 - 1) as u8,
        fp::GRID => l.grid = (l.grid as i32 + dir as i32).clamp(1, daw_engine::lfo::SH_GRID_MAX as i32) as u8,
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 100.0);
            }
        }
    }
}
