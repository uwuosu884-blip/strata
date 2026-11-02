//! Channel EQ (Live 12): response display with the HP switch and Mid
//! frequency, Low / Mid / High knobs, Output. Geometry measured from Live 12
//! at 100 % (l/t = device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv};
use super::modfx::drag_text;
use super::*;
use daw_engine::chaneq::*;
use daw_project::devices::params2::ceqp;

pub const WIDTH: f32 = 247.0;

/// Display: graph t+21..t+102, strip t+103..t+124; 0 dB at t+61, 32 px per
/// 12 dB; 100 Hz at l+59, 79.5 px per decade.
const PX_DB: f32 = 32.0 / 12.0;
const PX_DEC: f32 = 79.5;

fn gx(l: f32, f: f32) -> f32 {
    l + 59.5 + PX_DEC * (f / 100.0).log10()
}

fn gf(l: f32, x: f32) -> f32 {
    100.0 * 10f32.powf((x - l - 59.5) / PX_DEC)
}

fn freq_label(f: f32) -> String {
    if f >= 999.5 { format!("{:.2} kHz", f / 1000.0) } else { format!("{f:.0} Hz") }
}

/// Filled analyzer spectrum across `r` (`freq(x)` maps a pixel column to
/// Hz); nothing while the device is off or silent.
pub(super) fn spec_fill(cx: &mut Ctx, p: &Painter, r: Rect, col: Color32, freq: &dyn Fn(f32) -> f32) {
    let id = cx.dev.id;
    if !cx.on {
        cx.app.spectra.clear(id);
        return;
    }
    let sr = cx.app.spectra.sample_rate(id).max(8000.0);
    let spec = cx.app.spectra.update(id, 4096, 60.0, 2.0);
    let live = spec.live;
    let cols = r.width().max(1.0) as usize + 1;
    let db = crate::spectrum::curve_db(spec, sr, cols, |i| freq(r.left() + i));
    let mut mesh = egui::Mesh::default();
    let bottom = r.bottom();
    for (i, db) in db.iter().enumerate() {
        let x = r.left() + i as f32;
        let y = (bottom - (db + 84.0) / 84.0 * r.height()).clamp(r.top(), bottom);
        let v = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x, y), col);
        mesh.colored_vertex(pos2(x, bottom), col);
        if i > 0 {
            mesh.add_triangle(v - 2, v - 1, v);
            mesh.add_triangle(v - 1, v, v + 1);
        }
    }
    p.add(Shape::mesh(mesh));
    cx.app.animating |= live;
}

/// Knob centre marker (▼ above the ring).
fn marker(p: &Painter, x: f32, t: f32, col: Color32) {
    p.add(Shape::convex_polygon(vec![pos2(x - 3.5, t + 143.0), pos2(x + 3.5, t + 143.0), pos2(x, t + 147.0)], col, Stroke::NONE));
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut ChannelEqParams)) {
    if let FxParams::ChannelEq(c) = &mut cx.params {
        f(c);
    }
}

pub(super) fn channel_eq(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::ChannelEq(c0) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let on = cx.on;
    let disp = Rect::from_min_max(pos2(l + 4.0, t + 21.0), pos2(l + 243.0, t + 124.0));
    p.rect_filled(disp, CornerRadius::same(3), gray(0x18));
    let graph = Rect::from_min_max(disp.min, pos2(disp.right(), t + 102.0));
    let gp = p.with_clip_rect(graph.intersect(p.clip_rect()));
    spec_fill(cx, &gp, graph, gray(0x2e), &|x| gf(l, x));

    // Grid: decades full height above the labels, ±12 dB fainter.
    let bright = Color32::from_white_alpha(43);
    let fine = Color32::from_white_alpha(21);
    for f in [100.0f32, 1000.0, 10_000.0] {
        let x = gx(l, f).floor();
        quad(&gp, Rect::from_min_max(pos2(x, graph.top()), pos2(x + 1.0, t + 91.0)), bright);
    }
    let lab = gray(0x81);
    for (db, s) in [(12.0f32, "12"), (0.0, "0"), (-12.0, "-12")] {
        let y = t + 61.0 - db * PX_DB;
        quad(&gp, Rect::from_min_max(pos2(l + 26.0, y), pos2(graph.right(), y + 1.0)), if db == 0.0 { bright } else { fine });
        text_at(&gp, l + 22.0, y + 4.0, Align2::RIGHT_BOTTOM, s, font(FS), lab);
    }
    for (f, s) in [(100.0f32, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        let x = gx(l, f).floor() + 0.5;
        let tr = gp.layout_no_wrap(s.to_string(), font(FS), lab).size().x;
        // Labels knock the −12 dB line out (4 px either side).
        quad(&gp, Rect::from_min_max(pos2(x - tr / 2.0 - 4.0, t + 91.0), pos2(x + tr / 2.0 + 4.0, t + 101.0)), gray(0x18));
        text_at(&gp, x, t + 100.0, Align2::CENTER_BOTTOM, s, font(FS), lab);
    }
    // Response curve.
    let sr = cx.app.spectra.sample_rate(cx.dev.id).max(8000.0);
    let pts: Vec<Pos2> = (0..=((graph.width() as usize) * 2))
        .map(|i| {
            let x = graph.left() + i as f32 * 0.5;
            let db = c0.response_db(gf(l, x), sr);
            pos2(x, (t + 61.5 - db * PX_DB).clamp(graph.top() - 4.0, graph.bottom() + 4.0))
        })
        .collect();
    gp.add(Shape::line(pts, Stroke::new(1.5, if on { TEAL } else { gray(0x75) })));
    quad(p, Rect::from_min_max(pos2(disp.left(), t + 102.0), pos2(disp.right(), t + 103.0)), gray(0x3e));

    // HP 80 Hz switch (orange, high-pass glyph).
    let hb = Rect::from_min_size(pos2(l + 8.0, t + 107.0), vec2(21.0, 13.0));
    let (fill, glyph) = match (c0.hp, on) {
        (true, true) => (Some(ORANGE), gray(0x20)),
        (true, false) => (Some(gray(0x5a)), gray(0x24)),
        (false, true) => (None, ORANGE),
        (false, false) => (None, gray(0x5a)),
    };
    match fill {
        Some(f) => {
            p.rect_filled(hb, CornerRadius::ZERO, f);
        }
        None => {
            p.rect_stroke(hb, CornerRadius::ZERO, Stroke::new(1.0, glyph), egui::StrokeKind::Inside);
        }
    }
    let g = |x: f32, y: f32| pos2(hb.left() + x, hb.top() + y);
    p.add(Shape::line(
        vec![g(3.5, 9.5), g(5.0, 8.0), g(6.5, 6.5), g(8.5, 5.2), g(11.0, 4.2), g(13.5, 3.7), g(18.0, 3.5)],
        Stroke::new(1.2, glyph),
    ));
    if cx.toggle(ui, hb, ceqp::HP, true) {
        set(cx, |c| c.hp = !c.hp);
    }
    // Mid frequency (orange, centred over the Mid knob) and the ▼ notch.
    let s = freq_label(c0.mid_freq);
    let w = p.layout_no_wrap(s.clone(), font(FS), ORANGE).size().x;
    drag_text(cx, ui, p, l + 86.5 - w / 2.0, t + 117.0, ceqp::FREQ, &s, true, ORANGE);
    p.add(Shape::convex_polygon(
        vec![pos2(l + 83.0, t + 124.0), pos2(l + 90.0, t + 124.0), pos2(l + 86.5, t + 127.5)],
        gray(0x18),
        Stroke::NONE,
    ));

    // Knobs.
    let mk = if on { TEAL } else { gray(0x75) };
    for (x, k, label, v) in [
        (36.5, ceqp::LOW, "Low", c0.low),
        (86.5, ceqp::MID, "Mid", c0.mid),
        (136.5, ceqp::HIGH, "High", c0.high),
        (210.5, ceqp::OUTPUT, "Output", c0.output),
    ] {
        marker(p, l + x, t, if k == ceqp::OUTPUT { gray(0x11) } else { mk });
        knob_lv(cx, ui, p, l + x, t + 140.0, k, label, &format!("{v:.1} dB"), 0.5, KN, 21.5, 42.0, true);
    }
    quad(p, Rect::from_min_max(pos2(l + 173.0, t + 128.0), pos2(l + 174.0, t + 184.0)), gray(0x24));
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::ChannelEq(ChannelEqParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::ChannelEq(c) = p else { return };
    match k {
        ceqp::LOW => c.low = ((c.low * 10.0).round() + dir).clamp(-150.0, 150.0) / 10.0,
        ceqp::MID => c.mid = ((c.mid * 10.0).round() + dir).clamp(-120.0, 120.0) / 10.0,
        ceqp::HIGH => c.high = ((c.high * 10.0).round() + dir).clamp(-150.0, 150.0) / 10.0,
        ceqp::OUTPUT => c.output = ((c.output * 10.0).round() + dir).clamp(-120.0, 120.0) / 10.0,
        ceqp::HP => c.hp = dir > 0.0,
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 200.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        ceqp::LOW => "Low Gain",
        ceqp::MID => "Mid Gain",
        ceqp::FREQ => "Mid Freq",
        ceqp::HIGH => "High Gain",
        ceqp::OUTPUT => "Output",
        ceqp::HP => "HP On",
        _ => "",
    }
}
