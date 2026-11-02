//! Erosion (Live 12): input spectrum with the Freq / Amount handle (sine
//! line and noise band), Width / Stereo strip, Freq, Noise Blend and Amount
//! knobs. Geometry measured from Live 12 at 100 % (l/t = device left/top;
//! text positions are baselines).

use super::dynamics::{KN, KN_BIG, knob_lv};
use super::modfx::drag_text;
use super::*;
use daw_engine::erosion::*;
use daw_project::devices::params2::erp;

pub const WIDTH: f32 = 207.0;

/// Display x: 20 Hz at l+4 … 20 kHz at l+202 (66 px per decade).
const X0: f32 = 4.0;
const PX_DEC: f32 = 66.0;
/// Graph rows (Amount 0 at the bottom).
const G_TOP: f32 = 21.0;
const G_BOT: f32 = 90.0;
const LABEL: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);

fn gx(l: f32, f: f32) -> f32 {
    l + X0 + PX_DEC * (f / 20.0).log10()
}

fn gf(l: f32, x: f32) -> f32 {
    20.0 * 10f32.powf((x - l - X0) / PX_DEC)
}

fn hz(f: f32) -> String {
    if f >= 999.5 { format!("{:.2} kHz", f / 1000.0) } else { format!("{f:.0} Hz") }
}

/// Deterministic per-row dash phase.
fn row_phase(i: u32) -> f32 {
    ((i.wrapping_mul(2_654_435_761) >> 13) % 4) as f32
}

pub(super) fn erosion(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Erosion(e0) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let on = cx.on;
    let disp = Rect::from_min_max(pos2(l + 4.0, t + 21.0), pos2(l + 203.0, t + 112.0));
    p.rect_filled(disp, CornerRadius::same(3), gray(0x18));
    let graph = Rect::from_min_max(disp.min, pos2(disp.right(), t + G_BOT));
    let gp = p.with_clip_rect(graph.intersect(p.clip_rect()));
    // Decade lines behind the input spectrum.
    for f in [100.0f32, 1000.0, 10_000.0] {
        let x = gx(l, f).floor();
        quad(&gp, Rect::from_min_max(pos2(x, graph.top()), pos2(x + 1.0, graph.bottom())), Color32::from_white_alpha(43));
    }
    super::chaneq::spec_fill(cx, &gp, graph, gray(0x51), &|x| gf(l, x));

    // Handle: x = Freq, y = Amount.
    let teal = if on { TEAL } else { gray(0x86) };
    let fx = gx(l, e0.freq.clamp(ERO_FREQ.0, ERO_FREQ.1));
    let cy = t + G_BOT - e0.amount.clamp(0.0, 1.0) * (G_BOT - G_TOP);
    let blend = e0.blend.clamp(0.0, 1.0);
    // Noise band: dashed rows (3 on / 1 off) every 3 px below the handle,
    // fading out over ± (2 + 14 × Width) px.
    if blend > 0.0 {
        let hw = 2.0 + 14.0 * e0.width.clamp(ERO_WIDTH.0, ERO_WIDTH.1);
        let a_max = 0.72 * blend;
        let mut y = t + G_BOT - 1.0;
        let mut i = 0u32;
        let mut mesh = egui::Mesh::default();
        while y >= cy - 6.0 {
            let mut x = (fx - hw).floor() - row_phase(i);
            while x < fx + hw {
                for px in 0..3 {
                    let xx = x + px as f32;
                    let d = (xx + 0.5 - fx) / hw;
                    if d.abs() < 1.0 {
                        let a = a_max * (-1.8 * d * d).exp();
                        mesh.add_colored_rect(Rect::from_min_size(pos2(xx, y), vec2(1.0, 1.0)), teal.gamma_multiply(a));
                    }
                }
                x += 4.0;
            }
            y -= 3.0;
            i += 1;
        }
        gp.add(Shape::mesh(mesh));
    }
    // Sine: a 2 px line from the handle down, brighter towards the bottom.
    if blend < 1.0 {
        let mut mesh = egui::Mesh::default();
        let x = fx.floor() - 0.5;
        let top = cy + 6.5;
        let n = ((t + G_BOT) - top).max(0.0) as i32;
        for k in 0..n {
            let u = k as f32 / n.max(1) as f32;
            let a = (1.0 - blend) * (0.3 + 0.7 * u);
            mesh.add_colored_rect(Rect::from_min_size(pos2(x, top + k as f32), vec2(2.0, 1.0)), teal.gamma_multiply(a));
        }
        gp.add(Shape::mesh(mesh));
    }
    p.with_clip_rect(disp.intersect(p.clip_rect())).circle_stroke(pos2(fx + 0.5, cy + 0.5), 5.6, Stroke::new(1.6, teal));
    // Drag in the display: horizontal = Freq, vertical = Amount, Alt = Width.
    cx.salt = 1;
    let resp = cx.sense(ui, graph, erp::FREQ, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() || resp.clicked() {
        cx.focus(erp::FREQ);
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        let alt = ui.input(|i| i.modifiers.alt);
        if d != egui::Vec2::ZERO {
            cx.undo_step();
            if alt {
                let v = norm(&cx.params, erp::WIDTH).unwrap_or(0.0) - d.y / 100.0;
                set_norm(&mut cx.params, erp::WIDTH, v);
            } else {
                let fv = norm(&cx.params, erp::FREQ).unwrap_or(0.0);
                // Log axis: one pixel = 1/66 decade over the 30 Hz–18 kHz range.
                let span = (ERO_FREQ.1 / ERO_FREQ.0).log10() * PX_DEC;
                set_norm(&mut cx.params, erp::FREQ, fv + d.x / span);
                let av = norm(&cx.params, erp::AMOUNT).unwrap_or(0.0);
                set_norm(&mut cx.params, erp::AMOUNT, av - d.y / (G_BOT - G_TOP));
            }
        }
    }
    if resp.double_clicked() {
        reset(&mut cx.params, erp::FREQ);
        reset(&mut cx.params, erp::AMOUNT);
    }
    cx.param_menu(&resp, erp::FREQ);
    quad(p, Rect::from_min_max(pos2(disp.left(), t + 90.0), pos2(disp.right(), t + 91.0)), gray(0x3e));

    // Strip: Width (greyed without noise), sine / noise glyphs, Stereo.
    let w_en = blend > 0.0;
    text_at(p, l + 13.0, t + 105.0, Align2::LEFT_BOTTOM, "Width", font(FS), LABEL);
    drag_text(cx, ui, p, l + 48.0, t + 105.0, erp::WIDTH, &format!("{:.2}", e0.width), w_en, TEAL);
    let dim = Color32::from_rgb(20, 66, 71);
    let sine_c = if on { mix(dim, TEAL, 1.0 - blend) } else { gray(0x5a) };
    let noise_c = if on { mix(dim, TEAL, blend) } else { gray(0x5a) };
    let pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let u = i as f32 / 20.0;
            pos2(l + 91.0 + 9.5 * u, t + 101.5 - 2.2 * (u * std::f32::consts::TAU).sin())
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.2, sine_c)));
    for (dx, dy) in [(0.0, 0.0), (4.0, 0.0), (2.0, 2.0), (6.0, 2.0), (0.0, 4.0), (4.0, 4.0), (2.0, 6.0), (6.0, 6.0)] {
        quad(p, Rect::from_min_size(pos2(l + 108.0 + dx, t + 98.0 + dy), vec2(2.0, 1.0)), noise_c);
    }
    text_at(p, l + 131.0, t + 105.0, Align2::LEFT_BOTTOM, "Stereo", font(FS), LABEL);
    drag_text(cx, ui, p, l + 169.0, t + 105.0, erp::STEREO, &format!("{:.0} %", e0.stereo * 100.0), true, TEAL);

    // Knobs.
    knob_lv(cx, ui, p, l + 42.5, t + 135.0, erp::FREQ, "Freq", &hz(e0.freq), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 103.5, t + 128.0, erp::BLEND, "Noise Blend", &format!("{:.0} %", blend * 100.0), 0.0, KN_BIG, 21.5, 50.0, true);
    knob_lv(cx, ui, p, l + 164.5, t + 135.0, erp::AMOUNT, "Amount", &format!("{:.0} %", e0.amount * 100.0), 0.0, KN, 15.5, 36.0, true);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Erosion(ErosionParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Erosion(e) = p else { return };
    match k {
        erp::AMOUNT => e.amount = ((e.amount * 100.0).round() + dir).clamp(0.0, 100.0) / 100.0,
        erp::BLEND => e.blend = ((e.blend * 100.0).round() + dir).clamp(0.0, 100.0) / 100.0,
        erp::STEREO => e.stereo = ((e.stereo * 100.0).round() + dir).clamp(0.0, 100.0) / 100.0,
        erp::WIDTH => e.width = ((e.width * 100.0).round() + dir).clamp(10.0, 250.0) / 100.0,
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 200.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        erp::FREQ => "Frequency",
        erp::AMOUNT => "Amount",
        erp::BLEND => "Noise Blend",
        erp::WIDTH => "Filter Width",
        erp::STEREO => "Stereo Width",
        _ => "",
    }
}
