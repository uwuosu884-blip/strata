//! Shared pieces of the v0.8.9 devices (Resonators, Drum Buss, Spectral
//! Resonator, Spectral Time): panels, knob markers, the ♭♯ scale button,
//! LEDs, the display ▶ button and the spectrogram.

use super::*;

pub(super) const PURPLE: Color32 = Color32::from_rgb(0xa6, 0x8b, 0xf5);
use daw_project::devices::stft::{SG_COLS, SG_ROWS, spec_gram};

pub(super) fn rr(l: f32, t: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(l + x0, t + y0), pos2(l + x1, t + y1))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn px(p: &Painter, l: f32, t: f32, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
    quad(p, rr(l, t, x0, y0, x1, y1), c);
}

/// Body below the title in #46 with #36 panels `xs` (y 17..186, 2 px
/// rounded) — the panel layout of the spectral devices.
pub(super) fn panels(p: &Painter, r: Rect, xs: &[(f32, f32)], dark: &[(f32, f32)]) {
    let (l, t) = (r.left(), r.top());
    p.rect_filled(Rect::from_min_max(pos2(l, t + TITLE_H), r.max), CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, gray(0x46));
    for &(x0, x1) in xs {
        p.rect_filled(rr(l, t, x0, 17.0, x1, 186.0), CornerRadius::same(2), panel());
    }
    for &(x0, x1) in dark {
        p.rect_filled(rr(l, t, x0, 17.0, x1, 186.0), CornerRadius::same(2), gray(0x18));
    }
}

/// Toggle button whose selected-but-inactive look is Live's #75 fill with
/// dark text (Mono in Internal mode, Onsets in Manual mode).
#[allow(clippy::too_many_arguments)]
pub(super) fn sel_btn(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, label: &str, sel: bool, en: bool) -> bool {
    if en {
        return super::dynamics::tbutton(cx, ui, p, r, k, label, sel, true);
    }
    boxed(p, r, if sel { gray(0x75) } else { gray(0x1e) });
    text_at(p, r.center().x, r.top() + 10.0, Align2::CENTER_BOTTOM, label, font(FS), if sel { gray(0x36) } else { gray(0x75) });
    cx.toggle(ui, r, k, false)
}

/// ▼ centre marker above a knob ring centred at (`x`, `cy`).
pub(super) fn marker(p: &Painter, x: f32, cy: f32, col: Color32) {
    p.add(Shape::convex_polygon(vec![pos2(x - 3.5, cy - 18.5), pos2(x + 3.5, cy - 18.5), pos2(x, cy - 14.0)], col, Stroke::NONE));
}

/// Marker colour: teal off-centre, #11 at the centre, #2A disabled.
pub(super) fn marker_col(cx: &Ctx, en: bool, nonzero: bool) -> Color32 {
    match (en && cx.on, nonzero) {
        (false, _) => gray(0x2a),
        (true, true) => TEAL,
        (true, false) => gray(0x11),
    }
}

/// Title-bar ♭♯ (Scale Awareness / Use Current Scale) button, 13×13 at `x`.
pub(super) fn scale_btn(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, t: f32, k: u8, on: bool) -> bool {
    let br = Rect::from_min_size(pos2(x, t + 2.0), vec2(13.0, 13.0));
    let (bf, bc) = if on { (if cx.on { PURPLE } else { gray(0x75) }, gray(0x12)) } else { (gray(0x1e), text()) };
    boxed(p, br, bf);
    // ♭♯ glyph (pixel art as in Live).
    let px = |x0: f32, y0: f32, x1: f32, y1: f32| quad(p, Rect::from_min_max(pos2(x + x0, t + y0), pos2(x + x1, t + y1)), bc);
    px(2.0, 7.0, 3.0, 13.0);
    px(3.0, 9.0, 4.0, 10.0);
    px(4.0, 10.0, 5.0, 12.0);
    px(3.0, 12.0, 4.0, 13.0);
    px(7.0, 4.0, 8.0, 9.0);
    px(9.0, 4.0, 10.0, 9.0);
    px(6.0, 5.0, 11.0, 6.0);
    px(6.0, 7.0, 11.0, 8.0);
    cx.toggle(ui, br, k, true)
}

/// Limiter LED (Input Send).
pub(super) fn led(p: &Painter, c: Pos2, lit: bool) {
    let col = if lit { Color32::from_rgb(0xff, 0x5a, 0x3a) } else { Color32::from_rgb(0xc3, 0x5f, 0x40) };
    p.circle_filled(c, 3.6, col);
}

/// Display ▶ button (orange circle): toggles the spectrogram.
pub(super) fn play_btn(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, c: Pos2, k: u8, on: bool) -> bool {
    let col = if !cx.on {
        gray(0x75)
    } else if on {
        ORANGE
    } else {
        Color32::from_rgb(0xaf, 0x7a, 0x41)
    };
    p.circle_stroke(c, 5.6, Stroke::new(1.2, col));
    p.add(Shape::convex_polygon(vec![pos2(c.x - 2.0, c.y - 3.5), pos2(c.x + 3.5, c.y), pos2(c.x - 2.0, c.y + 3.5)], col, Stroke::NONE));
    cx.toggle(ui, Rect::from_center_size(c, vec2(14.0, 14.0)), k, true)
}

/// Orange checkbox (7×7 outline, filled when on) with a grey label.
#[allow(clippy::too_many_arguments)]
pub(super) fn check(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, label: &str, on: bool) -> bool {
    let col = if cx.on { ORANGE } else { gray(0x75) };
    let b = Rect::from_min_size(pos2(x, base - 7.0), vec2(7.0, 7.0));
    if on {
        p.rect_filled(b, CornerRadius::ZERO, col);
    } else {
        p.rect_stroke(b, CornerRadius::ZERO, Stroke::new(1.0, col), egui::StrokeKind::Inside);
    }
    let tr = text_at(p, x + 12.0, base, Align2::LEFT_BOTTOM, label, font(FS), gray(0x86));
    cx.toggle(ui, Rect::from_min_max(pos2(x - 2.0, base - 10.0), pos2(tr.right() + 2.0, base + 3.0)), k, true)
}

/// Spectrogram of device `id` in `r` (newest column at the right): dry in
/// yellow, wet in blue, additive on #18.
pub(super) fn spectrogram(cx: &mut Ctx, ui: &egui::Ui, p: &Painter, r: Rect, running: bool) {
    let g = spec_gram(cx.dev.id);
    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = (((r.width() * ppp).round() as usize).max(1), ((r.height() * ppp).round() as usize).max(1));
    let end = g.written();
    let cols = SG_COLS.min(w);
    let mut img = egui::ColorImage::new([w, h], vec![gray(0x18); w * h]);
    const DRY: (f32, f32, f32) = (0.80, 0.76, 0.45);
    const WET: (f32, f32, f32) = (0.30, 0.78, 0.98);
    let bg = 0x18 as f32 / 255.0;
    for x in 0..w {
        // Columns stretched to the width, oldest at the left.
        let ci = x * cols / w;
        let Some(abs) = (end + ci).checked_sub(cols) else { continue };
        if end == 0 || abs >= end {
            continue;
        }
        for y in 0..h {
            let k = (h - 1 - y) * SG_ROWS / h;
            let (d, wv) = g.get(abs, k);
            let (d, wv) = (d * d, wv.powf(1.6));
            let ch = |a: f32, b: f32| ((bg + d * a * 0.55 + wv * b * 0.85).min(1.0) * 255.0) as u8;
            img.pixels[y * w + x] = Color32::from_rgb(ch(DRY.0, WET.0), ch(DRY.1, WET.1), ch(DRY.2, WET.2));
        }
    }
    let id = egui::Id::unique(("spectrogram", cx.dev.id));
    let tex = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(id));
    let tex = match tex {
        Some(mut t) => {
            t.set(img, egui::TextureOptions::NEAREST);
            t
        }
        None => {
            let t = ui.ctx().load_texture(format!("spectrogram-{}", cx.dev.id), img, egui::TextureOptions::NEAREST);
            ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
            t
        }
    };
    p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    if running && cx.on {
        cx.app.animating = true;
    }
}

/// Note / dB / Hz / % / ms value strings.
pub(super) fn db2(x: f32) -> String {
    format!("{} dB", super::dynamics::sig3(x))
}

pub(super) fn hz(f: f32) -> String {
    if f >= 999.5 {
        format!("{} kHz", super::dynamics::sig3(f / 1000.0))
    } else if f >= 99.95 {
        format!("{f:.0} Hz")
    } else {
        format!("{f:.1} Hz")
    }
}

pub(super) fn ms(x: f32) -> String {
    if x >= 1000.0 { format!("{} s", super::dynamics::sig3(x / 1000.0)) } else { format!("{} ms", super::dynamics::sig3(x)) }
}

pub(super) fn st0(x: f32) -> String {
    format!("{} st", x.round() as i32)
}
