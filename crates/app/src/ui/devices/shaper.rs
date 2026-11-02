//! Shaper (Live 12 Max for Live device): a breakpoint envelope that runs as
//! an LFO (`LfoParams::shaper`). Map / Multimap like LFO; Jitter, Smooth,
//! Phase, Offset, Loop / 1-Shot / Manual, Rate, Depth. Geometry measured
//! from Live 12 at 100 % (l/t = device left/top; text positions are
//! baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::lfo::{LABEL, lfo_of, map_row, metronome, multimap_button, multimap_list, pct, scope};
use super::modfx::drag_text;
use super::*;
use daw_engine::lfo::{LFO_SYNC, SH_GRID_MAX, SH_PTS, ShPt, ShTrig, sh_preset};
use daw_project::devices::LfoParams;

pub const WIDTH: f32 = 445.0;
const ICON: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
/// Breakpoint editor: value 1 at `T1`, 0 at `T0`; cycle from `X0` over `XW`.
const X0: f32 = 8.5;
const XW: f32 = 272.0;
const T1: f32 = 41.5;
const T0: f32 = 162.5;

#[derive(Clone, Copy)]
enum Drag {
    Point(usize),
    Curve { seg: usize, c0: f32 },
}

fn gx(l: f32, x: f32) -> f32 {
    l + X0 + x * XW
}
fn gy(t: f32, y: f32) -> f32 {
    t + T0 - y * (T0 - T1)
}

/// Snap `x` to the grid when Snap is on.
fn snap(l0: &LfoParams, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if l0.snap {
        let g = l0.grid.clamp(1, SH_GRID_MAX) as f32;
        (x * g).round() / g
    } else {
        x
    }
}

/// Miniature of a shape (preset icons) in `r`.
fn shape_icon(p: &Painter, r: Rect, l: &LfoParams, col: Color32) {
    let n = r.width() as usize * 2;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let x = i as f32 / n as f32;
            pos2(r.left() + x * r.width(), r.bottom() - l.shaper_at(x as f64) * r.height())
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.0, col)));
}

pub(super) fn shaper(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Lfo(l0) = cx.params else { return };
    let on = cx.on;
    let accent = if on { ORANGE } else { gray(0x75) };

    // ---- left: map row + breakpoint editor, or the multimap list ----
    if l0.multimap {
        multimap_list(cx, ui, p, l, t, l + 286.0, l + 268.0, 175.0);
    } else {
        let d = Rect::from_min_max(pos2(l + 4.0, t + 21.0), pos2(l + 286.0, t + 184.0));
        p.rect_filled(d, CornerRadius::same(2), gray(0x18));
        map_row(cx, ui, p, l, t + 25.0, 0);
        multimap_button(cx, ui, p, l + 268.0, t, false);
        editor(cx, ui, p, l, t, &l0, accent);
        // Clear + presets.
        let cy = t + 172.0;
        let xc = pos2(l + 14.0, cy);
        let s = Stroke::new(1.0, ICON);
        p.line_segment([xc + vec2(-3.0, -3.0), xc + vec2(3.0, 3.0)], s);
        p.line_segment([xc + vec2(-3.0, 3.0), xc + vec2(3.0, -3.0)], s);
        let mut pick = None;
        cx.salt = 0;
        if cx.toggle(ui, Rect::from_center_size(xc, vec2(12.0, 13.0)), fp::PRESET, true) {
            pick = Some(0);
        }
        for (i, x) in [39.0, 64.5, 90.5, 117.0, 144.0, 169.0].into_iter().enumerate() {
            let r = Rect::from_center_size(pos2(l + x, cy), vec2(19.0, 11.0));
            let (pts, npts) = sh_preset(i + 1);
            shape_icon(p, r, &LfoParams { pts, npts, ..l0 }, ICON);
            cx.salt = i as u32 + 1;
            if cx.toggle(ui, r.expand(1.0), fp::PRESET, true) {
                pick = Some(i + 1);
            }
        }
        cx.salt = 0;
        if let Some(i) = pick
            && let Some(l) = lfo_of(&mut cx.params)
        {
            (l.pts, l.npts) = sh_preset(i);
        }
        text_at(p, l + 197.0, t + 177.0, Align2::LEFT_BOTTOM, "Grid", font(FS), LABEL);
        drag_text(cx, ui, p, l + 222.0, t + 177.0, fp::GRID, &l0.grid.to_string(), true, ORANGE);
        let sr = Rect::from_min_max(pos2(l + 253.0, t + 167.0), pos2(l + 282.0, t + 180.0));
        let tc = if l0.snap {
            p.rect_filled(sr, CornerRadius::ZERO, accent);
            gray(0x20)
        } else {
            p.rect_stroke(sr, CornerRadius::ZERO, Stroke::new(1.0, accent), egui::StrokeKind::Inside);
            accent
        };
        text_at(p, sr.center().x, sr.top() + 10.0, Align2::CENTER_BOTTOM, "Snap", font(FS), tc);
        if cx.toggle(ui, sr, fp::SNAP, true)
            && let Some(l) = lfo_of(&mut cx.params)
        {
            l.snap = !l.snap;
        }
    }

    // ---- right top: scope + Jitter / Smooth / R / Phase / Offset ----
    let d = Rect::from_min_max(pos2(l + 290.0, t + 21.0), pos2(l + 441.0, t + 102.0));
    p.rect_filled(d, CornerRadius::same(2), gray(0x18));
    let cy = t + 44.0;
    let line_y = (cy - l0.offset * 19.0).round() + 0.5;
    quad(p, Rect::from_min_max(pos2(d.left(), line_y - 0.5), pos2(d.right(), line_y + 0.5)), gray(0x3e));
    let sd = Rect::from_min_max(d.min, pos2(d.right(), t + 68.0));
    let still = |ph: f64| match l0.trig {
        ShTrig::Manual => l0.shaper_at(l0.phase as f64) * 2.0 - 1.0,
        _ => l0.shaper_at(ph.rem_euclid(1.0)) * 2.0 - 1.0,
    };
    // Live shows ~4.3 s (8.6 cycles of 1/4 at 120 BPM).
    scope(cx, p, sd, cy, 19.0, 4.3, &l0, still);
    let val =
        |cx: &mut Ctx, ui: &mut egui::Ui, x: f32, base: f32, k: u8, s: &str| drag_text(cx, ui, p, l + x, t + base, k, s, true, ORANGE);
    text_at(p, l + 301.0, t + 79.0, Align2::LEFT_BOTTOM, "Jitter", font(FS), LABEL);
    val(cx, ui, 331.0, 79.0, fp::JITTER, &pct(l0.jitter));
    text_at(p, l + 370.0, t + 79.0, Align2::LEFT_BOTTOM, "Smooth", font(FS), LABEL);
    val(cx, ui, 409.0, 79.0, fp::SMOOTH, &pct(l0.smooth));
    // R: retrigger (greyed while synced, except 1-Shot).
    let r_en = !l0.sync || l0.trig == ShTrig::OneShot;
    let rc = pos2(l + 300.5, t + 91.0);
    let rcol = if r_en && on { text() } else { gray(0x55) };
    p.circle_stroke(rc, 5.5, Stroke::new(1.0, rcol));
    text_at(p, rc.x, rc.y + 3.5, Align2::CENTER_BOTTOM, "R", font(FS_SMALL), rcol);
    if cx.toggle(ui, Rect::from_center_size(rc, vec2(13.0, 13.0)), fp::RETRIG, r_en)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.retrig = l.retrig.wrapping_add(1);
    }
    // Phase (Ø).
    let pc = pos2(l + 320.0, t + 91.0);
    p.circle_stroke(pc, 3.0, Stroke::new(1.0, LABEL));
    p.line_segment([pc + vec2(-3.5, 3.5), pc + vec2(3.5, -3.5)], Stroke::new(1.0, LABEL));
    val(cx, ui, 331.0, 95.0, fp::PHASE, &format!("{:.2} %", l0.phase * 100.0));
    text_at(p, l + 370.0, t + 95.0, Align2::LEFT_BOTTOM, "Offset", font(FS), LABEL);
    val(cx, ui, 409.0, 95.0, fp::OFFSET, &format!("{:.0} %", l0.offset * 100.0));

    // ---- right bottom: mode, rate, depth ----
    for (i, (m, label, x)) in
        [(ShTrig::Loop, "Loop", 296.0), (ShTrig::OneShot, "1-Shot", 343.0), (ShTrig::Manual, "Manual", 390.0)].into_iter().enumerate()
    {
        cx.salt = i as u32;
        if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + x, t + 112.0), vec2(44.0, 13.0)), fp::TRIG, label, l0.trig == m, true)
            && let Some(l) = lfo_of(&mut cx.params)
        {
            l.trig = m;
            l.retrig = l.retrig.wrapping_add(1);
        }
    }
    cx.salt = 0;
    let rate =
        if l0.sync { LFO_SYNC[(l0.sync_rate as usize).min(LFO_SYNC.len() - 1)].1.to_string() } else { format!("{} Hz", sig3(l0.rate)) };
    let manual = l0.trig == ShTrig::Manual;
    let rk = if l0.sync { fp::SYNC_RATE } else { fp::RATE };
    knob_lv(cx, ui, p, l + 316.5, t + 139.0, rk, "Rate", &rate, 0.0, KN, 17.5, 37.0, !manual);
    quad(p, Rect::from_min_max(pos2(l + 333.0, t + 157.0), pos2(l + 340.0, t + 158.0)), gray(0x55));
    let hzr = Rect::from_min_max(pos2(l + 343.0, t + 142.0), pos2(l + 363.0, t + 155.0));
    toggle_look(p, hzr, "Hz", !l0.sync, true, on);
    if cx.toggle(ui, hzr, fp::SYNC, true)
        && let Some(l) = lfo_of(&mut cx.params)
    {
        l.sync = false;
    }
    let nr = Rect::from_min_max(pos2(l + 343.0, t + 158.0), pos2(l + 363.0, t + 171.0));
    toggle_look(p, nr, "", l0.sync, true, on);
    metronome(p, nr.center(), if l0.sync { gray(0x20) } else { text() });
    cx.salt = 1;
    let hit = cx.toggle(ui, nr, fp::SYNC, true);
    cx.salt = 0;
    if hit && let Some(l) = lfo_of(&mut cx.params) {
        l.sync = true;
    }
    knob_lv(cx, ui, p, l + 411.0, t + 139.0, fp::DEPTH, "Depth", &pct(l0.depth), 0.0, KN, 17.5, 37.0, true);
}

/// Breakpoint editor: click adds, drag moves, Shift+click deletes,
/// Alt+drag bends the segment under the pointer.
fn editor(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, l0: &LfoParams, accent: Color32) {
    let line = gray(0x31);
    let g = l0.grid.clamp(1, SH_GRID_MAX);
    // Frame and grid: 2 px lines (Live draws them at 2x).
    let vl = |x: f32| quad(p, Rect::from_min_max(pos2(x - 1.0, t + T1 - 1.0), pos2(x + 1.0, t + T0 + 1.0)), line);
    for i in 0..=g {
        vl(gx(l, i as f32 / g as f32));
    }
    for y in [T1, T0] {
        quad(p, Rect::from_min_max(pos2(gx(l, 0.0) - 1.0, t + y - 1.0), pos2(gx(l, 1.0) + 1.0, t + y + 1.0)), line);
    }
    let mid = gy(t, 0.5).floor();
    quad(p, Rect::from_min_max(pos2(gx(l, 0.0) + 1.0, mid), pos2(gx(l, 1.0) - 1.0, mid + 1.0)), gray(0x4c));
    // Curve and points.
    let n = (l0.npts as usize).min(SH_PTS);
    let steps = (XW * 2.0) as usize;
    let mut pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let x = i as f32 / steps as f32;
            pos2(gx(l, x), gy(t, l0.shaper_at(x as f64)))
        })
        .collect();
    // Vertical jumps (points sharing an x) as exact verticals.
    if n > 0 {
        let first = l0.pts[0];
        pts.insert(0, pos2(gx(l, first.x), gy(t, first.y)));
    }
    p.add(Shape::line(pts, Stroke::new(1.5, accent)));
    for q in &l0.pts[..n] {
        p.circle_filled(pos2(gx(l, q.x), gy(t, q.y)), 2.5, accent);
    }

    // Interaction.
    let area = Rect::from_min_max(pos2(l + 4.0, t + T1 - 5.0), pos2(l + 286.0, t + T0 + 4.0));
    let resp = ui.interact(area, egui::Id::unique(("sh_edit", cx.dev.id)), Sense::click_and_drag());
    let did = egui::Id::unique(("sh_drag", cx.dev.id));
    let to_val = |pos: Pos2| ((pos.x - l - X0) / XW, ((t + T0 - pos.y) / (T0 - T1)).clamp(0.0, 1.0));
    let hit = |pos: Pos2| -> Option<usize> {
        (0..n).filter(|&i| pos2(gx(l, l0.pts[i].x), gy(t, l0.pts[i].y)).distance(pos) <= 5.0).min_by(|&a, &b| {
            let da = pos2(gx(l, l0.pts[a].x), gy(t, l0.pts[a].y)).distance(pos);
            let db = pos2(gx(l, l0.pts[b].x), gy(t, l0.pts[b].y)).distance(pos);
            da.total_cmp(&db)
        })
    };
    let mods = ui.input(|i| i.modifiers);
    if resp.hovered() && hit(resp.hover_pos().unwrap_or_default()).is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let insert = |l: &mut LfoParams, x: f32, y: f32| -> Option<usize> {
        let n = (l.npts as usize).min(SH_PTS);
        if n >= SH_PTS {
            return None;
        }
        let at = l.pts[..n].iter().position(|q| q.x > x).unwrap_or(n);
        l.pts.copy_within(at..n, at + 1);
        l.pts[at] = ShPt { x, y, c: 0.0 };
        l.npts += 1;
        Some(at)
    };
    if resp.clicked() || resp.drag_started() {
        cx.focus(fp::POINTS);
    }
    if resp.clicked() {
        let pos = resp.interact_pointer_pos().unwrap_or_default();
        cx.undo_step();
        match hit(pos) {
            Some(i) if mods.shift => {
                if let Some(l) = lfo_of(&mut cx.params) {
                    let n = l.npts as usize;
                    l.pts.copy_within(i + 1..n, i);
                    l.npts -= 1;
                }
            }
            Some(_) => {}
            None if !mods.alt => {
                let (x, y) = to_val(pos);
                let x = snap(l0, x);
                if let Some(l) = lfo_of(&mut cx.params) {
                    insert(l, x, y);
                }
            }
            None => {}
        }
    }
    if resp.drag_started() {
        let pos = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
        cx.undo_step();
        let (x, y) = to_val(pos);
        let drag = if mods.alt {
            // The segment under the pointer.
            let seg = l0.pts[..n].iter().rposition(|q| q.x <= x.clamp(0.0, 1.0)).filter(|&s| s + 1 < n);
            seg.map(|seg| Drag::Curve { seg, c0: l0.pts[seg].c })
        } else if let Some(i) = hit(pos) {
            Some(Drag::Point(i))
        } else {
            let x = snap(l0, x);
            lfo_of(&mut cx.params).and_then(|l| insert(l, x, y)).map(Drag::Point)
        };
        ui.data_mut(|d| match drag {
            Some(v) => {
                d.insert_temp(did, v);
            }
            None => d.remove::<Drag>(did),
        });
    }
    if resp.dragged()
        && let Some(drag) = ui.data(|d| d.get_temp::<Drag>(did))
        && let Some(pos) = resp.interact_pointer_pos()
    {
        match drag {
            Drag::Point(i) => {
                let (x, y) = to_val(pos);
                let x = snap(l0, x);
                if let Some(l) = lfo_of(&mut cx.params) {
                    let n = l.npts as usize;
                    if i < n {
                        let lo = if i > 0 { l.pts[i - 1].x } else { 0.0 };
                        let hi = if i + 1 < n { l.pts[i + 1].x } else { 1.0 };
                        l.pts[i].x = x.clamp(lo, hi);
                        l.pts[i].y = y;
                    }
                }
            }
            Drag::Curve { seg, c0 } => {
                let dy = pos.y - ui.input(|i| i.pointer.press_origin()).map_or(pos.y, |o| o.y);
                if let Some(l) = lfo_of(&mut cx.params)
                    && seg + 1 < l.npts as usize
                {
                    let dir = if l.pts[seg + 1].y >= l.pts[seg].y { 1.0 } else { -1.0 };
                    l.pts[seg].c = (c0 + dy * 0.01 * dir).clamp(-1.0, 1.0);
                }
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Drag>(did));
    }
    if cx.focused(fp::POINTS) {
        brackets(p, area.shrink2(vec2(2.0, 0.0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaper_editor_maps_match_live() {
        // Live: cycle 0 / 1 at x 17.5 / 289.5, value 1 / 0 at y 47.5 / 168.5 (l = 9, t = 6).
        assert_eq!(gx(9.0, 0.0), 17.5);
        assert_eq!(gx(9.0, 1.0), 289.5);
        assert_eq!(gy(6.0, 1.0), 47.5);
        assert_eq!(gy(6.0, 0.0), 168.5);
        let l = LfoParams { grid: 4, snap: true, ..LfoParams::shaper() };
        assert_eq!(snap(&l, 0.3), 0.25);
        assert_eq!(snap(&LfoParams { snap: false, ..l }, 0.3), 0.3);
    }
}
