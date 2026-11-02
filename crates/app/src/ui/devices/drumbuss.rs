//! Drum Buss (Live 12): Drive / type / Trim / Comp column, Crunch / Damp /
//! Transients, Boom / Freq / Decay / Force To Note / Audition, and the dark
//! Bass + Output meter panel with Dry/Wet. Geometry measured from Live 12 at
//! 100 % (l/t = device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv, tbutton};
use super::fx4::*;
use super::modfx::drag_text;
use super::*;
use daw_project::devices::dev_tap;
use daw_project::devices::drumbuss::*;
use daw_project::devices::params4::dbp;

pub const WIDTH: f32 = 267.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut DrumBussParams)) {
    if let FxParams::DrumBuss(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn pct(x: f32) -> String {
    format!("{:.0} %", x * 100.0)
}

fn pct1(x: f32) -> String {
    format!("{:.1} %", x * 100.0)
}

fn out_s(x: f32) -> String {
    if x <= DB_OUT.0 + 1e-3 { "-inf dB".into() } else { db2(x) }
}

/// Meter y (device coordinates) for a level in dB: 0 dB on the #3E line at
/// y 47, −60 dB at the bottom (146).
fn meter_y(db: f32) -> f32 {
    if db >= 0.0 { (47.0 - db * 8.0 / 6.0).max(39.0) } else { (47.0 - db / 60.0 * 99.0).min(146.0) }
}

/// Meter gradient (top red → orange → yellow-green → green).
fn grad(y: f32) -> Color32 {
    const STOPS: [(f32, [u8; 3]); 4] =
        [(48.0, [0xce, 0x2b, 0x05]), (61.0, [0xf6, 0x9d, 0x00]), (82.0, [0xc8, 0xe2, 0x00]), (95.0, [0x00, 0xf7, 0x58])];
    if y <= STOPS[0].0 {
        let c = STOPS[0].1;
        return Color32::from_rgb(c[0], c[1], c[2]);
    }
    for w in STOPS.windows(2) {
        let ((y0, a), (y1, b)) = (w[0], w[1]);
        if y <= y1 {
            let u = (y - y0) / (y1 - y0);
            let m = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * u) as u8;
            return Color32::from_rgb(m(0), m(1), m(2));
        }
    }
    let c = STOPS[3].1;
    Color32::from_rgb(c[0], c[1], c[2])
}

#[derive(Clone, Copy, Default)]
struct Meters {
    out: [f32; 2],
    bass: f32,
    hold: f32,
    hold_t: f32,
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::DrumBuss(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    px(p, l, t, 58.0, 24.0, 59.0, 180.0, gray(0x24));
    px(p, l, t, 118.0, 24.0, 119.0, 180.0, gray(0x24));

    // Drive column.
    knob_lv(cx, ui, p, l + 28.5, t + 31.0, dbp::DRIVE, "Drive", &pct(d.drive), 0.0, KN, 14.5, 36.0, true);
    for (i, y) in [75.0, 91.0, 107.0].into_iter().enumerate() {
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr(l, t, 5.0, y, 52.0, y + 13.0), dbp::TYPE, DB_TYPES[i], d.dtype == i as u8, true) {
            set(cx, |q| q.dtype = i as u8);
        }
        cx.salt = 0;
    }
    text_at(p, l + 6.0, t + 137.0, Align2::LEFT_BOTTOM, "Trim", font(FS), text());
    value_box(cx, ui, p, rr(l, t, 5.0, 142.0, 52.0, 155.0), dbp::TRIM, &db2(d.trim), true);
    if tbutton(cx, ui, p, rr(l, t, 5.0, 168.0, 52.0, 181.0), dbp::COMP, "Comp", d.comp, true) {
        set(cx, |q| q.comp = !q.comp);
    }

    // Crunch / Damp / Transients.
    knob_lv(cx, ui, p, l + 88.5, t + 31.0, dbp::CRUNCH, "Crunch", &pct1(d.crunch), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 88.5, t + 84.0, dbp::DAMP, "Damp", &hz(d.damp), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 88.5, t + 137.0, dbp::TRANSIENTS, "Transients", &format!("{:.2}", d.transients), 0.5, KN, 17.5, 42.0, true);
    marker(p, l + 88.5, t + 154.5, marker_col(cx, true, d.transients.abs() > 0.005));

    // Boom.
    knob_lv(cx, ui, p, l + 147.5, t + 31.0, dbp::BOOM, "Boom", &pct1(d.boom), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 147.5, t + 84.0, dbp::FREQ, "Freq", &hz(d.freq), 0.0, KN, 14.5, 36.0, true);
    text_at(p, l + 125.0, t + 137.0, Align2::LEFT_BOTTOM, "Decay", font(FS), text());
    value_box(cx, ui, p, rr(l, t, 124.0, 142.0, 171.0, 155.0), dbp::DECAY, &pct(d.decay), true);
    let (_, note) = force_note(d.freq);
    let pill = rr(l, t, 124.0, 168.0, 154.0, 181.0);
    p.rect_filled(pill, CornerRadius::same(7), gray(0x11));
    p.rect_filled(pill.shrink(1.0), CornerRadius::same(6), gray(0x1e));
    text_at(p, pill.center().x, t + 178.0, Align2::CENTER_BOTTOM, &note, font(FS), if cx.on { text() } else { dim() });
    if cx.toggle(ui, pill, dbp::FORCE, true) {
        set(cx, |q| q.freq = 440.0 * 2f32.powf((force_note(q.freq).0 as f32 - 69.0) / 12.0));
    }
    // Boom Audition (headphones).
    let hc = pos2(l + 163.5, t + 174.5);
    let col = match (d.audition, cx.on) {
        (true, true) => ORANGE,
        (true, false) => gray(0x75),
        _ => text(),
    };
    p.circle_filled(hc, 6.6, gray(0x11));
    p.circle_filled(hc, 5.6, gray(0x1e));
    p.add(Shape::line(
        (0..=12).map(|i| hc + vec2(-(PI * i as f32 / 12.0).cos() * 3.0, -(PI * i as f32 / 12.0).sin() * 3.0 + 0.5)).collect(),
        Stroke::new(1.0, col),
    ));
    quad(p, Rect::from_min_max(hc + vec2(-3.5, 0.5), hc + vec2(-1.5, 4.0)), col);
    quad(p, Rect::from_min_max(hc + vec2(1.5, 0.5), hc + vec2(3.5, 4.0)), col);
    if cx.toggle(ui, Rect::from_center_size(hc, vec2(14.0, 14.0)), dbp::AUDITION, true) {
        set(cx, |q| q.audition = !q.audition);
    }

    // Meter panel.
    p.rect_filled(rr(l, t, 176.0, 21.0, 264.0, 183.0), CornerRadius::same(2), gray(0x18));
    px(p, l, t, 176.0, 164.0, 264.0, 165.0, gray(0x3e));
    text_at(p, l + 201.5, t + 34.0, Align2::CENTER_BOTTOM, "Bass", font(FS), gray(0x77));
    text_at(p, l + 240.0, t + 34.0, Align2::CENTER_BOTTOM, "Out", font(FS), gray(0x77));

    let tap = dev_tap(cx.dev.id);
    let mid = egui::Id::unique(("db_meter", cx.dev.id));
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let mut m: Meters = ui.data(|dd| dd.get_temp(mid)).unwrap_or_default();
    let fall = 10f32.powf(-24.0 * dt / 20.0);
    for (c, k) in TAP_OUT.into_iter().enumerate() {
        m.out[c] = tap.take(k, 0.0).max(m.out[c] * fall);
    }
    m.bass = tap.take(TAP_BASS, 0.0).max(m.bass * fall).min(1.0);
    if m.bass >= m.hold {
        (m.hold, m.hold_t) = (m.bass, 0.0);
    } else {
        m.hold_t += dt;
        if m.hold_t > 1.5 {
            m.hold = m.bass;
        }
    }
    if cx.on && (m.out[0] > 1e-4 || m.out[1] > 1e-4 || m.bass > 1e-4) {
        cx.app.animating = true;
    }
    ui.data_mut(|dd| dd.insert_temp(mid, m));

    // Bass meter: #3E frame, orange fill, hold line, percent.
    let frame = |x0: f32| {
        p.rect_stroke(rr(l, t, x0, 38.0, x0 + 11.0, 147.0), CornerRadius::ZERO, Stroke::new(1.0, gray(0x3e)), egui::StrokeKind::Inside);
    };
    frame(195.0);
    let bass_col = if cx.on { Color32::from_rgb(0xff, 0xa4, 0x18) } else { gray(0x75) };
    let by = 146.0 - m.bass * 107.0;
    if m.bass > 0.005 {
        px(p, l, t, 196.0, by, 205.0, 146.0, bass_col);
        px(p, l, t, 196.0, by - 1.0, 205.0, by, mix(bass_col, Color32::WHITE, 0.35));
    }
    if m.hold > 0.005 {
        let hy = (146.0 - m.hold * 107.0).round();
        px(p, l, t, 196.0, hy, 205.0, hy + 1.0, bass_col);
    }
    text_at(p, l + 200.0, t + 158.0, Align2::CENTER_BOTTOM, &format!("{:.1} %", m.bass * 100.0), font(FS), gray(0x7f));

    // Output meter: two channels with the gradient, 0 dB line, ◀ fader.
    frame(234.0);
    px(p, l, t, 235.0, 47.0, 244.0, 48.0, gray(0x3e));
    px(p, l, t, 239.0, 39.0, 240.0, 146.0, gray(0x18));
    for (c, x0) in [235.0, 240.0].into_iter().enumerate() {
        let db = 20.0 * m.out[c].max(1e-6).log10();
        let top = meter_y(db).round();
        if top >= 146.0 {
            continue;
        }
        let mut y = top.max(39.0);
        while y < 146.0 {
            if y != 47.0 {
                let col = if cx.on { grad(y) } else { gray(0x75) };
                px(p, l, t, x0, y, x0 + 4.0, y + 1.0, col);
            }
            y += 1.0;
        }
    }
    let fy = t + meter_y(d.output);
    let tri_col = if cx.on { gray(0x86) } else { gray(0x5a) };
    p.add(Shape::convex_polygon(vec![pos2(l + 246.0, fy), pos2(l + 256.0, fy - 5.5), pos2(l + 256.0, fy + 5.5)], tri_col, Stroke::NONE));
    cx.drag(ui, rr(l, t, 232.0, 36.0, 258.0, 149.0), dbp::OUTPUT, true);
    let os = out_s(d.output);
    let w = p.layout_no_wrap(os.clone(), font(FS), Color32::WHITE).size().x;
    cx.salt = 1;
    drag_text(cx, ui, p, l + 240.0 - w * 0.5, t + 158.0, dbp::OUTPUT, &os, true, Color32::from_rgb(0xe6, 0x9d, 0x4f));
    cx.salt = 0;

    text_at(p, l + 182.0, t + 178.0, Align2::LEFT_BOTTOM, "Dry / Wet", font(FS), gray(0x86));
    drag_text(cx, ui, p, l + 233.0, t + 178.0, dbp::DRY_WET, &pct(d.dry_wet), true, Color32::from_rgb(0x06, 0xa6, 0xb5));
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::DrumBuss(DrumBussParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::DrumBuss(d) = p else { return };
    match k {
        dbp::TRIM => d.trim = ((d.trim * 10.0).round() + dir).clamp(DB_TRIM.0 * 10.0, DB_TRIM.1 * 10.0) / 10.0,
        dbp::OUTPUT => d.output = ((d.output * 10.0).round() + dir).clamp(DB_OUT.0 * 10.0, DB_OUT.1 * 10.0) / 10.0,
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, ((v * 100.0).round() + dir) / 100.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        dbp::TRIM => "Trim",
        dbp::DRIVE => "Drive",
        dbp::CRUNCH => "Crunch",
        dbp::DAMP => "Damping Freq",
        dbp::TRANSIENTS => "Transients",
        dbp::BOOM => "Boom Amt",
        dbp::FREQ => "Boom Freq",
        dbp::DECAY => "Boom Decay",
        dbp::OUTPUT => "Output Gain",
        dbp::DRY_WET => "Dry/Wet",
        dbp::COMP => "Compressor On",
        dbp::TYPE => "Drive Type",
        dbp::FORCE => "Force To Note",
        dbp::AUDITION => "Boom Audition",
        _ => "",
    }
}
