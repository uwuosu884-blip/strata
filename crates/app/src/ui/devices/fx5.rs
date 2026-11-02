//! Shared pieces of the v0.8.10 devices (Dynamic Tube, Vocoder, Vinyl
//! Distortion, Shifter): XY dragging on displays and translucent grids.

use super::*;

/// Relative XY drag over `r`: x moves `kx` by `dx / wx`, y moves `ky` by
/// `-dy / wy`; with Alt held the vertical motion edits `alt` instead
/// (Vinyl Distortion's Q). Shift = fine.
#[allow(clippy::too_many_arguments)]
pub(super) fn xy_drag(cx: &mut Ctx, ui: &egui::Ui, r: Rect, kx: u8, ky: u8, alt: Option<u8>, wx: f32, wy: f32) {
    // Own salt: the same parameter also has a number box.
    let salt = cx.salt;
    cx.salt = salt | 0x1000;
    let resp = cx.sense(ui, r, kx, Sense::click_and_drag());
    cx.salt = salt;
    if resp.drag_started() || resp.clicked() {
        cx.focus(kx);
    }
    let d = resp.drag_delta();
    if d == egui::Vec2::ZERO {
        return;
    }
    cx.undo_step();
    let (fine, alt_on) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let s = if fine { 0.1 } else { 1.0 };
    let mut bump = |k: u8, dv: f32| {
        if let Some(v) = norm(&cx.params, k) {
            set_norm(&mut cx.params, k, (v + dv * s).clamp(0.0, 1.0));
        }
    };
    match alt.filter(|_| alt_on) {
        Some(ka) => bump(ka, -d.y / wy),
        None => {
            bump(kx, d.x / wx);
            bump(ky, -d.y / wy);
        }
    }
}

/// 1-px translucent white line (grids drawn over #18: crossings add up).
pub(super) fn gline(p: &Painter, r: Rect, a: f32) {
    p.rect_filled(r, CornerRadius::ZERO, Color32::from_white_alpha((a * 255.0).round() as u8));
}

/// Alpha bitmap glyph (sampled from Live at 100 %) at (`x`, `y`).
pub(super) fn glyph<const W: usize, const H: usize>(p: &Painter, x: f32, y: f32, g: &[[u8; W]; H], col: Color32) {
    // One unfeathered mesh: exact pixels.
    let mut m = egui::Mesh::default();
    for (j, row) in g.iter().enumerate() {
        for (i, &a) in row.iter().enumerate() {
            if a > 0 {
                let r = Rect::from_min_size(pos2(x + i as f32, y + j as f32), vec2(1.0, 1.0));
                m.add_colored_rect(r, col.gamma_multiply(a as f32 / 255.0));
            }
        }
    }
    p.add(Shape::mesh(m));
}

/// Live's synced-time (metronome) glyph, 9×9.
pub(super) const METRO: [[u8; 9]; 9] = [
    [0, 0, 0, 0, 0, 79, 144, 0, 0],
    [0, 0, 0, 0, 0, 159, 96, 0, 0],
    [0, 0, 111, 255, 0, 238, 32, 0, 0],
    [0, 0, 223, 47, 79, 191, 0, 0, 0],
    [0, 47, 223, 0, 159, 96, 96, 79, 0],
    [0, 144, 144, 0, 208, 32, 111, 159, 0],
    [0, 223, 47, 0, 0, 0, 15, 238, 0],
    [47, 223, 0, 0, 0, 0, 0, 191, 64],
    [96, 255, 255, 255, 255, 255, 255, 255, 79],
];

/// Shifter's phase / spin icon, 9×9.
pub(super) const PHASE_ICON: [[u8; 9]; 9] = [
    [0, 16, 128, 223, 255, 223, 128, 16, 0],
    [16, 206, 144, 32, 0, 49, 160, 206, 16],
    [128, 255, 255, 206, 32, 0, 0, 144, 128],
    [223, 128, 0, 128, 190, 0, 0, 32, 223],
    [255, 0, 0, 0, 255, 0, 0, 0, 255],
    [223, 128, 0, 128, 206, 0, 0, 49, 223],
    [128, 255, 255, 190, 32, 0, 0, 160, 128],
    [16, 206, 160, 49, 0, 32, 144, 206, 16],
    [0, 16, 128, 223, 255, 223, 128, 16, 0],
];
