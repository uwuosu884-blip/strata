//! Tuner (Live 12): Classic view (Target / Strobe arc, note + cents / Hz
//! readout) and Histogram view (pitch history over note lanes, Auto),
//! view icons, ct / Hz, Reference, spelling context menu. Geometry measured
//! from Live 12 at 100 % (l/t = device left/top; text positions are baselines).

use super::fx4::{px, rr};
use super::modfx::drag_text;
use super::*;
use daw_project::devices::params6::tup;
use daw_project::devices::tuner::*;

pub const WIDTH: f32 = 308.0;

const GREEN: Color32 = Color32::from_rgb(0x4f, 0xc7, 0x7b);
const ARC_GREEN: Color32 = Color32::from_rgb(0x29, 0xbb, 0x8a);
const RED: Color32 = Color32::from_rgb(0xff, 0x74, 0x57);
const HIST_ORANGE: Color32 = Color32::from_rgb(0xff, 0x9e, 0x56);
const HIST_BROWN: Color32 = Color32::from_rgb(0x8b, 0x5b, 0x37);
/// Cents counted as in tune.
const IN_TUNE: f32 = 5.0;
/// Histogram: pixels per semitone; frames per dash slot; slot pitch.
const LANE: f32 = 41.0;
const SLOT_FRAMES: usize = 6;
const SLOT_PX: f32 = 21.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut TunerParams)) {
    if let FxParams::Tuner(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

/// Note `n` (MIDI) → (letter, accidental 0 none / 1 sharp / 2 flat) for a spelling.
fn spell(n: i32, flats: bool) -> (&'static str, u8) {
    const SH: [(&str, u8); 12] =
        [("C", 0), ("C", 1), ("D", 0), ("D", 1), ("E", 0), ("F", 0), ("F", 1), ("G", 0), ("G", 1), ("A", 0), ("A", 1), ("B", 0)];
    const FL: [(&str, u8); 12] =
        [("C", 0), ("D", 2), ("D", 0), ("E", 2), ("E", 0), ("F", 0), ("G", 2), ("G", 0), ("A", 2), ("A", 0), ("B", 2), ("B", 0)];
    let i = n.rem_euclid(12) as usize;
    if flats { FL[i] } else { SH[i] }
}

/// Painted ♯ / ♭ with its top-left at (x, top), `h` px tall; returns the width.
fn accidental(p: &Painter, x: f32, top: f32, h: f32, kind: u8, col: Color32) -> f32 {
    let s = Stroke::new((h / 11.0).max(1.0), col);
    match kind {
        1 => {
            let w = h * 0.55;
            for dx in [0.3, 0.7] {
                p.line_segment([pos2(x + w * dx, top), pos2(x + w * dx, top + h)], s);
            }
            let sk = Stroke::new((h / 7.0).max(1.2), col);
            for dy in [0.35, 0.68] {
                p.line_segment([pos2(x, top + h * (dy + 0.06)), pos2(x + w, top + h * (dy - 0.06))], sk);
            }
            w
        }
        2 => {
            let w = h * 0.5;
            p.line_segment([pos2(x + 1.0, top), pos2(x + 1.0, top + h)], s);
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| i as f32 / 12.0 * PI)
                .map(|a| pos2(x + 1.0 + (w - 1.0) * a.sin(), top + h * 0.78 - h * 0.22 * a.cos()))
                .collect();
            p.add(Shape::line(pts, s));
            w
        }
        _ => 0.0,
    }
}

/// Note name (letter + accidental, both spellings when `spelling` = 2) at
/// (x, base) with font size `fs`; `right` aligns its right edge to `x`.
#[allow(clippy::too_many_arguments)]
fn note_name(p: &Painter, x: f32, base: f32, n: i32, spelling: u8, fs: f32, col: Color32, center: bool) -> f32 {
    let f = font(fs);
    let parts: Vec<(&str, u8)> = match spelling {
        1 => vec![spell(n, true)],
        2 if spell(n, false).1 != 0 => vec![spell(n, false), spell(n, true)],
        _ => vec![spell(n, false)],
    };
    let cap = fs * 0.7;
    let acc_h = cap * 0.72;
    let measure = |p: &Painter| -> f32 {
        let mut w = 0.0;
        for (i, (l, a)) in parts.iter().enumerate() {
            if i > 0 {
                w += p.layout_no_wrap("/".into(), f.clone(), col).size().x;
            }
            w += p.layout_no_wrap(l.to_string(), f.clone(), col).size().x;
            if *a != 0 {
                w += acc_h * 0.6 + fs * 0.08;
            }
        }
        w
    };
    let w = measure(p);
    let mut cx = if center { x - w / 2.0 } else { x };
    for (i, (l, a)) in parts.iter().enumerate() {
        if i > 0 {
            cx = text_at(p, cx, base, Align2::LEFT_BOTTOM, "/", f.clone(), col).right();
        }
        cx = text_at(p, cx, base, Align2::LEFT_BOTTOM, l, f.clone(), col).right();
        if *a != 0 {
            cx += fs * 0.08;
            cx += accidental(p, cx, base - cap - cap * 0.05, acc_h, *a, col);
        }
    }
    w
}

/// Flat 10 px display button (no border).
#[allow(clippy::too_many_arguments)]
fn flat_btn(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, label: &str, fill: Color32) -> bool {
    let fill = if cx.on { fill } else { mix(fill, gray(0x18), 0.5) };
    p.rect_filled(r, CornerRadius::ZERO, fill);
    text_at(p, r.center().x, r.bottom() - 1.5, Align2::CENTER_BOTTOM, label, font(FS), gray(0x20));
    cx.toggle(ui, r, k, true)
}

fn arc_y(x: f32) -> f32 {
    let (cx0, cy0, rad) = (155.0, 369.6, 298.6f32);
    cy0 - (rad * rad - (x - cx0) * (x - cx0)).max(0.0).sqrt()
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Tuner(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let disp = rr(l, t, 4.0, 21.0, 304.0, 184.0);
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));
    let tap = dev_tap(cx.dev.id);
    let freq = if cx.on { tap.get(TAP_FREQ) } else { 0.0 };
    cx.app.animating = true;
    let a4 = d.reference.clamp(TU_REF.0, TU_REF.1);
    let has = freq > 0.0;
    let (note, cents) = if has { note_of(freq, a4) } else { (69, 0.0) };
    let tuned = has && cents.abs() < IN_TUNE;
    let col = if tuned { GREEN } else { RED };

    // Right-click: note spelling.
    let resp = ui.interact(rr(l, t, 4.0, 21.0, 304.0, 164.0), egui::Id::unique(("tuner_disp", cx.dev.id)), Sense::click_and_drag());
    let mut pick = None;
    menu::context(&resp, 223.0, |ui| {
        for (i, s) in TU_SPELL.iter().enumerate() {
            if menu::check_item(ui, s, "", d.spelling == i as u8, true) {
                pick = Some(i as u8);
            }
        }
    });
    if let Some(i) = pick {
        set(cx, |q| q.spelling = i);
    }

    if d.histogram {
        histogram(cx, p, l, t, &d, &tap, &resp);
    } else {
        // Arc, target ring, ball.
        let arc_col = if tuned { ARC_GREEN } else { gray(0x86) };
        let pts: Vec<Pos2> = (0..=113).map(|i| 41.0 + i as f32 * 2.0).map(|x| pos2(l + x, t + arc_y(x))).collect();
        p.add(Shape::line(pts, Stroke::new(1.0, arc_col)));
        let lab = gray(0x86);
        text_at(p, l + 154.0, t + 47.0, Align2::CENTER_BOTTOM, "0", font(FS), lab);
        text_at(p, l + 14.0, t + 103.0, Align2::LEFT_BOTTOM, "-50", font(FS), lab);
        text_at(p, l + 294.0, t + 103.0, Align2::RIGHT_BOTTOM, "+50", font(FS), lab);
        if d.strobe {
            // Strobe: dashes on the arc drifting at a speed set by the detune.
            let time = ui.input(|i| i.time) as f32;
            let off = if has { (time * cents * 0.6).rem_euclid(1.0) } else { 0.0 };
            let c = if has { col } else { gray(0x4a) };
            for i in 0..12 {
                let x0 = 41.0 + ((i as f32 + off) * 20.0).rem_euclid(240.0);
                let x1 = (x0 + 9.0).min(267.0);
                if x0 >= 267.0 {
                    continue;
                }
                let seg: Vec<Pos2> = (0..=4).map(|j| x0 + (x1 - x0) * j as f32 / 4.0).map(|x| pos2(l + x, t + arc_y(x) - 4.0)).collect();
                p.add(Shape::line(seg, Stroke::new(4.0, c)));
            }
        }
        let ring = pos2(l + 154.5, t + 71.0);
        p.circle_filled(ring, 11.5, gray(0x18));
        p.circle_stroke(ring, 11.5, Stroke::new(1.0, arc_col));
        if has && !d.strobe {
            let bx = 155.0 + cents.clamp(-50.0, 50.0) * 2.2;
            p.circle_filled(pos2(l + bx, t + arc_y(bx)), 10.0, col);
        }
        // Readout.
        if has {
            let fs = 23.0;
            note_name(p, l + 86.0, t + 135.0, note, d.spelling, fs, col, false);
            let v = if d.hz { format!("{freq:.1} Hz") } else { format!("{cents:+.1} ct") };
            text_at(p, l + 219.0, t + 135.0, Align2::RIGHT_BOTTOM, &v, font(fs), col);
        }
    }

    // Bottom row.
    px(p, l, t, 4.0, 164.0, 304.0, 165.0, gray(0x3e));
    for x in [24.0, 44.0] {
        px(p, l, t, x, 164.0, x + 1.0, 184.0, gray(0x3e));
    }
    let on_c = if cx.on { ORANGE } else { gray(0x75) };
    let off_c = gray(0x77);
    // Classic icon: small arc; Histogram icon: pitch trace.
    let ic = if d.histogram { off_c } else { on_c };
    let arc_pts: Vec<Pos2> = (0..=8).map(|i| i as f32 / 8.0).map(|u| pos2(l + 10.0 + 8.0 * u, t + 173.5 - 2.5 * (u * PI).sin())).collect();
    p.add(Shape::line(arc_pts, Stroke::new(1.5, ic)));
    p.line_segment([pos2(l + 10.0, t + 173.0), pos2(l + 10.0, t + 175.0)], Stroke::new(1.5, ic));
    p.line_segment([pos2(l + 18.0, t + 173.0), pos2(l + 18.0, t + 175.0)], Stroke::new(1.5, ic));
    let hc = if d.histogram { on_c } else { off_c };
    let tr = [(30.0, 174.5), (32.0, 174.5), (33.5, 171.0), (35.5, 177.0), (37.0, 174.5), (40.0, 174.5)];
    p.add(Shape::line(tr.iter().map(|&(x, y)| pos2(l + x, t + y)).collect(), Stroke::new(1.2, hc)));
    if cx.toggle(ui, rr(l, t, 4.0, 165.0, 24.0, 184.0), tup::VIEW, true) && d.histogram {
        set(cx, |q| q.histogram = false);
    }
    cx.salt = 1;
    if cx.toggle(ui, rr(l, t, 25.0, 165.0, 44.0, 184.0), tup::VIEW, true) && !d.histogram {
        set(cx, |q| q.histogram = true);
    }
    cx.salt = 0;
    if d.histogram {
        let fill = if d.auto { TEAL } else { gray(0x24) };
        if flat_btn(cx, ui, p, rr(l, t, 52.0, 169.0, 88.0, 180.0), tup::AUTO, "Auto", fill) {
            set(cx, |q| q.auto = !q.auto);
        }
    } else if flat_btn(cx, ui, p, rr(l, t, 52.0, 169.0, 88.0, 180.0), tup::STROBE, if d.strobe { "Strobe" } else { "Target" }, ORANGE) {
        set(cx, |q| q.strobe = !q.strobe);
    }
    if flat_btn(cx, ui, p, rr(l, t, 94.0, 169.0, 111.0, 180.0), tup::HZ, if d.hz { "Hz" } else { "ct" }, ORANGE) {
        set(cx, |q| q.hz = !q.hz);
    }
    text_at(p, l + 205.0, t + 179.0, Align2::LEFT_BOTTOM, "Reference", font(FS), gray(0x86));
    let s = format!("{:.0} Hz", d.reference);
    let w = p.layout_no_wrap(s.clone(), font(FS), TEAL).size().x;
    drag_text(cx, ui, p, l + 293.0 - w, t + 179.0, tup::REFERENCE, &s, true, TEAL);
}

/// Histogram view: note lanes, the pitch history as dashes, note column.
fn histogram(cx: &mut Ctx, p: &Painter, l: f32, t: f32, d: &TunerParams, tap: &DevTap, resp: &Response) {
    let a4 = d.reference.clamp(TU_REF.0, TU_REF.1);
    let freq = if cx.on { tap.get(TAP_FREQ) } else { 0.0 };
    let has = freq > 0.0;
    // Centre note: the detected note (Auto) or the scrolled position.
    let mut center = d.center;
    if d.auto && has {
        center = note_of(freq, a4).0 as f32;
    }
    if !d.auto && resp.dragged() {
        let dy = resp.drag_delta().y;
        if dy != 0.0
            && let FxParams::Tuner(q) = &mut cx.params
        {
            q.center = (q.center + dy / LANE).clamp(12.0, 120.0);
        }
    } else if d.auto && has && (d.center - center).abs() > 0.5 {
        // Keep the stored centre with the pitch so switching Auto off holds it.
        if let FxParams::Tuner(q) = &mut cx.params {
            q.center = center;
        }
    }
    let cy = 96.0;
    let y_of = |m: f32| cy - (m - center) * LANE;
    let clip = p.with_clip_rect(rr(l, t, 4.0, 21.0, 304.0, 164.0));
    let c0 = center.round() as i32;
    let cur = if has { Some(note_of(freq, a4)) } else { None };
    for n in (c0 - 3)..=(c0 + 3) {
        let y = y_of(n as f32);
        px(&clip, l, t, 4.0, y.round(), 277.0, y.round() + 1.0, gray(0x2b));
        let (fill, lab) = match cur {
            Some((cn, _)) if cn == n => (Some(HIST_ORANGE), gray(0x20)),
            Some((cn, c)) if c.abs() >= 1.0 && n == cn + c.signum() as i32 => (Some(HIST_BROWN), gray(0x20)),
            _ => (None, gray(0x75)),
        };
        if let Some(f) = fill {
            clip.rect_filled(rr(l, t, 277.0, y - 20.5, 304.0, y + 20.5), CornerRadius::ZERO, f);
        }
        note_name(&clip, l + 290.5, t + y + 6.0, n, d.spelling, 12.0, lab, true);
    }
    // History: one dash per slot of frames, newest at the right.
    let total = tap.written();
    if total > 0 {
        let slots = 14usize;
        let frames = slots * SLOT_FRAMES + (total % SLOT_FRAMES);
        let h = tap.history(frames.min(total));
        let n = h.len();
        let first = total - n;
        let newest_slot = (total - 1) / SLOT_FRAMES;
        let mut slot = first / SLOT_FRAMES;
        while slot <= newest_slot {
            let a = (slot * SLOT_FRAMES).max(first) - first;
            let b = ((slot + 1) * SLOT_FRAMES - first).min(n);
            let fr: Vec<f32> = h[a..b].iter().map(|f| f[0]).filter(|f| *f > 0.0).collect();
            if let (Some(&f0), Some(&f1)) = (fr.first(), fr.last()) {
                let m = |f: f32| 69.0 + 12.0 * (f / a4).log2();
                let x1 = 277.0 - (newest_slot - slot) as f32 * SLOT_PX - 4.0;
                clip.line_segment([pos2(l + x1 - 10.0, t + y_of(m(f0))), pos2(l + x1, t + y_of(m(f1)))], Stroke::new(2.0, gray(0x86)));
            }
            slot += 1;
        }
    }
    if let Some((_, c)) = cur {
        let v = if d.hz { format!("{freq:.1} Hz") } else { format!("{c:+.1} ct") };
        text_at(&clip, l + 132.0, t + 120.0, Align2::CENTER_BOTTOM, &v, bold(12.0), HIST_ORANGE);
    }
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    if k == tup::REFERENCE
        && let FxParams::Tuner(d) = p
    {
        d.reference = 440.0;
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    if k == tup::REFERENCE
        && let FxParams::Tuner(d) = p
    {
        d.reference = (d.reference + dir).clamp(TU_REF.0, TU_REF.1);
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        tup::REFERENCE => "Reference",
        tup::VIEW => "View",
        tup::STROBE => "Classic Mode",
        tup::HZ => "Display Unit",
        tup::AUTO => "Auto",
        _ => "",
    }
}
