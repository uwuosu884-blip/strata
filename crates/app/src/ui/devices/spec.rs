//! Spectrum (Live 12): Block / Channel / Refresh / Avg / Graph / Scale X /
//! range settings and the analyzer display, which can unfold into the main
//! window. Geometry measured from Live 12 at 100 % (l/t = device left/top;
//! text positions are baselines).

use super::dynamics::sig3;
use super::eq8::note_name;
use super::*;
use daw_engine::spectrum::{SPEC_BLOCKS, SPEC_DB, SpecChannel, SpecScale, SpectrumParams};
use daw_project::devices::params::spp;

pub const SPEC_W: f32 = 404.0;
/// Display unfolded into the main window: settings only.
pub const SPEC_FOLDED_W: f32 = 124.0;
const LOW_HZ: f32 = 10.0;
/// Max hold line.
const MAX_COL: Color32 = Color32::from_rgb(1, 97, 106);

pub fn width(s: &SpectrumParams) -> f32 {
    if s.expanded { SPEC_FOLDED_W } else { SPEC_W }
}

fn sp(cx: &Ctx) -> SpectrumParams {
    match cx.params {
        FxParams::Spectrum(s) => s,
        _ => SpectrumParams::default(),
    }
}

fn with(cx: &mut Ctx, f: impl FnOnce(&mut SpectrumParams)) {
    if let FxParams::Spectrum(s) = &mut cx.params {
        f(s);
    }
}

/// Teal toggle (Auto).
fn teal_button(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, label: &str, on: bool) -> bool {
    let (fill, col) = match (on, cx.on) {
        (true, true) => (TEAL, Color32::from_rgb(0x20, 0x20, 0x20)),
        (true, false) => (gray(0x5a), gray(0x24)),
        (false, _) => (gray(0x1e), if cx.on { text() } else { gray(0x69) }),
    };
    boxed(p, r, fill);
    text_at(p, r.center().x, r.top() + 10.0, Align2::CENTER_BOTTOM, label, font(FS), col);
    cx.toggle(ui, r, k, true)
}

pub(super) fn spectrum(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let s0 = sp(cx);
    let (l, t) = (r.left(), r.top());
    p.rect_filled(Rect::from_min_max(pos2(l, t + 17.0), r.max), CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, panel());
    for y in [45.0, 75.0, 124.0] {
        quad(p, Rect::from_min_max(pos2(l + 4.0, t + y), pos2(l + 118.0, t + y + 1.0)), gray(0x24));
    }
    let lab = |s: &str, base: f32| {
        text_at(p, l + 5.0, t + base, Align2::LEFT_BOTTOM, s, font(FS), text());
    };
    let rr = |x: f32, y: f32, w: f32| Rect::from_min_size(pos2(l + x, t + y), vec2(w, 13.0));

    // Block chooser.
    lab("Block", 33.0);
    let br = rr(46.0, 24.0, 73.0);
    boxed(p, br, box_fill(cx.on));
    let col = if cx.on { text() } else { dim() };
    text_at(p, br.left() + 6.0, br.top() + 10.0, Align2::LEFT_BOTTOM, &s0.block.to_string(), font(FS), col);
    p.add(Shape::convex_polygon(
        vec![pos2(br.right() - 8.0, br.top() + 4.0), pos2(br.right() - 2.0, br.top() + 4.0), pos2(br.right() - 5.0, br.top() + 8.0)],
        col,
        Stroke::NONE,
    ));
    let resp = cx.sense(ui, br, spp::BLOCK, Sense::click());
    cx.param_menu(&resp, spp::BLOCK);
    if resp.clicked() {
        cx.focus(spp::BLOCK);
    }
    let mut pick = None;
    menu::dropdown_aligned(&resp, br.width(), egui::RectAlign::BOTTOM_START, |ui| {
        for (i, b) in SPEC_BLOCKS.iter().enumerate() {
            if menu::check_item(ui, &b.to_string(), "", *b == s0.block, true) {
                pick = Some(i);
            }
        }
    });
    if let Some(i) = pick {
        cx.undo_step();
        with(cx, |s| s.block = SPEC_BLOCKS[i]);
    }
    if cx.focused(spp::BLOCK) {
        brackets(p, br.expand(1.0));
    }

    // Channel.
    lab("Channel", 63.0);
    for (i, (c, name, x, w)) in
        [(SpecChannel::L, "L", 46.0, 13.0), (SpecChannel::R, "R", 62.0, 13.0), (SpecChannel::LR, "L+R", 78.0, 22.0)].into_iter().enumerate()
    {
        cx.salt = i as u32;
        if super::dynamics::tbutton(cx, ui, p, rr(x, 54.0, w), spp::CHANNEL, name, s0.channel == c, true) {
            with(cx, |s| s.channel = c);
        }
    }
    cx.salt = 0;

    // Refresh / Avg.
    lab("Refresh", 94.0);
    value_box(cx, ui, p, rr(46.0, 84.0, 50.0), spp::REFRESH, &format!("{:.1} ms", s0.refresh), true);
    lab("Avg", 112.0);
    value_box(cx, ui, p, rr(46.0, 103.0, 50.0), spp::AVG, &format!("{:.0}", s0.avg), true);

    // Graph / Max.
    lab("Graph", 143.0);
    if super::dynamics::tbutton(cx, ui, p, rr(46.0, 133.0, 38.0), spp::LINE, if s0.line { "Line" } else { "Bins" }, s0.line, true) {
        with(cx, |s| s.line = !s.line);
    }
    if super::dynamics::tbutton(cx, ui, p, rr(87.0, 133.0, 31.0), spp::MAX, "Max", s0.max, true) {
        with(cx, |s| s.max = !s.max);
        cx.app.spectra.reset_peak(cx.dev.id);
    }

    // Scale X.
    lab("Scale X", 161.0);
    for (i, (sc, name, x)) in
        [(SpecScale::Lin, "Lin", 46.0), (SpecScale::Log, "Log", 71.0), (SpecScale::St, "ST", 96.0)].into_iter().enumerate()
    {
        cx.salt = i as u32;
        if super::dynamics::tbutton(cx, ui, p, rr(x, 151.0, 22.0), spp::SCALE, name, s0.scale == sc, true) {
            with(cx, |s| s.scale = sc);
        }
    }
    cx.salt = 0;

    // Auto + range.
    if teal_button(cx, ui, p, rr(5.0, 169.0, 34.0), spp::AUTO, "Auto", s0.auto) {
        with(cx, |s| s.auto = !s.auto);
    }
    let (hi, lo) = shown_range(ui, cx.dev.id, &s0);
    for (k, v, x) in [(spp::HI, hi, 46.0), (spp::LO, lo, 86.0)] {
        let b = rr(x, 169.0, 33.0);
        boxed(p, b, if s0.auto || !cx.on { panel() } else { box_fill(true) });
        text_at(p, b.center().x, b.top() + 10.0, Align2::CENTER_BOTTOM, &format!("{:.0}", v), font(FS), col);
        let before = sp(cx);
        if s0.auto {
            // Dragging a box takes over the shown auto range.
            with(cx, |s| (s.hi, s.lo) = (hi, lo));
        }
        cx.drag(ui, b, k, true);
        let after = sp(cx);
        if (after.hi, after.lo) != (hi, lo) {
            with(cx, |s| s.auto = false);
        } else if s0.auto {
            with(cx, |s| (s.hi, s.lo) = (before.hi, before.lo));
        }
        if cx.focused(k) {
            brackets(p, b.expand(1.0));
        }
    }
    quad(p, Rect::from_min_max(pos2(l + 81.0, t + 173.0), pos2(l + 84.0, t + 175.0)), col);

    if !s0.expanded {
        let d = Rect::from_min_max(pos2(l + 124.0, t + 22.0), pos2(l + 400.0, t + 184.0));
        let pp = p.clone();
        display(cx, ui, &pp, d, false);
    }
}

/// The range in use: the stored one, or the auto range (eased per frame).
fn shown_range(ui: &egui::Ui, id: u64, s: &SpectrumParams) -> (f32, f32) {
    if !s.auto {
        return (s.hi, s.lo);
    }
    ui.data(|d| d.get_temp::<(f32, f32)>(egui::Id::unique(("spec_auto", id)))).unwrap_or((s.hi, s.lo))
}

/// Frequency axis of a display.
struct Ax {
    r: Rect,
    scale: SpecScale,
    nyq: f32,
}

impl Ax {
    fn x(&self, f: f32) -> f32 {
        let u = match self.scale {
            SpecScale::Lin => f / self.nyq,
            _ => (f.max(1.0) / LOW_HZ).log10() / (self.nyq / LOW_HZ).log10(),
        };
        self.r.left() + u * self.r.width()
    }
    fn f(&self, x: f32) -> f32 {
        let u = (x - self.r.left()) / self.r.width();
        match self.scale {
            SpecScale::Lin => (u * self.nyq).max(0.0),
            _ => LOW_HZ * (self.nyq / LOW_HZ).powf(u),
        }
    }
}

/// Analyzer display (device or main window).
fn display(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, big: bool) {
    let s0 = sp(cx);
    let id = cx.dev.id;
    p.rect_filled(r, CornerRadius::same(2), gray(0x18));
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    let sr = cx.app.spectra.sample_rate(id).max(8000.0);
    let ax = Ax { r, scale: s0.scale, nyq: sr * 0.5 };
    let cols = r.width().max(1.0) as usize + 1;

    // Levels per pixel column (live and max hold).
    let (live, peak, alive) = if cx.on {
        let spec = cx.app.spectra.update_blocks(id, s0.block, s0.refresh, s0.avg);
        let alive = spec.live;
        let col_f = |i: usize, d: f32| ax.f(r.left() + i as f32 + d).max(1.0);
        let band = |i: usize| -> (f32, f32) {
            match s0.scale {
                SpecScale::St => {
                    // The semitone holding the column centre.
                    let m = (69.0 + 12.0 * (col_f(i, 0.0) / 440.0).log2()).round();
                    let fc = 440.0 * 2f32.powf((m - 69.0) / 12.0);
                    (fc * 2f32.powf(-1.0 / 24.0), fc * 2f32.powf(1.0 / 24.0))
                }
                _ => (col_f(i, -0.5), col_f(i, 0.5)),
            }
        };
        let get = |peak: bool| -> Vec<f32> {
            if s0.line && s0.scale != SpecScale::St {
                if peak {
                    (0..cols).map(|i| spec.peak_db(col_f(i, -0.5), col_f(i, 0.5), sr)).collect()
                } else {
                    (0..cols).map(|i| spec.level_db(col_f(i, -0.5), col_f(i, 0.5), sr)).collect()
                }
            } else {
                (0..cols)
                    .map(|i| {
                        let (a, b) = band(i);
                        spec.bin_db(peak, a, b, sr)
                    })
                    .collect()
            }
        };
        let lv = get(false);
        let pk = if s0.max { get(true) } else { Vec::new() };
        (lv, pk, alive)
    } else {
        cx.app.spectra.clear(id);
        (Vec::new(), Vec::new(), false)
    };
    cx.app.animating |= alive;

    // Auto range: follows the loudest and quietest shown levels.
    let aid = egui::Id::unique(("spec_auto", id));
    if s0.auto {
        let src = if peak.is_empty() { &live } else { &peak };
        let mx = src.iter().cloned().fold(f32::MIN, f32::max);
        let mn = src.iter().cloned().filter(|d| *d > -150.0).fold(f32::MAX, f32::min);
        let cur = ui.data(|d| d.get_temp::<(f32, f32)>(aid)).unwrap_or((s0.hi, s0.lo));
        let tgt = if mx > -150.0 && mn < f32::MAX {
            let hi = (mx + 2.0).ceil().clamp(SPEC_DB.0 + 12.0, SPEC_DB.1);
            (hi, (mn - 2.0).floor().clamp(SPEC_DB.0, hi - 12.0))
        } else {
            cur
        };
        let k = 0.15;
        let next = ((cur.0 + (tgt.0 - cur.0) * k).round(), (cur.1 + (tgt.1 - cur.1) * k).round());
        let next = if (next.0 - cur.0).abs() < 1.0 && tgt.0 != cur.0 { (cur.0 + (tgt.0 - cur.0).signum(), next.1) } else { next };
        let next = if (next.1 - cur.1).abs() < 1.0 && tgt.1 != cur.1 { (next.0, cur.1 + (tgt.1 - cur.1).signum()) } else { next };
        if next != cur {
            cx.app.animating = true;
        }
        ui.data_mut(|d| d.insert_temp(aid, next));
    }
    let (hi, lo) = shown_range(ui, id, &s0);
    let span = (hi - lo).max(1.0);
    let y_of = |db: f32| r.top() + (hi - db) / span * r.height();

    // Grid.
    let fine = gray(0x2b);
    let bright = gray(0x3f);
    let vline = |x: f32, c: Color32| {
        let x = x.floor();
        if x > r.left() && x < r.right() - 1.0 {
            p.rect_filled(Rect::from_min_max(pos2(x, r.top()), pos2(x + 1.0, r.bottom())), CornerRadius::ZERO, c);
        }
    };
    let mut flabels: Vec<(f32, String)> = Vec::new();
    match s0.scale {
        SpecScale::Log => {
            for d in [10.0f32, 100.0, 1000.0, 10_000.0] {
                for m in 1..=9 {
                    let f = d * m as f32;
                    if f > LOW_HZ && f < ax.nyq {
                        vline(ax.x(f), if m == 1 { bright } else { fine });
                    }
                }
            }
            flabels = vec![(100.0, "100".into()), (1000.0, "1k".into()), (10_000.0, "10k".into())];
        }
        SpecScale::Lin => {
            let mut f = 1000.0;
            while f < ax.nyq {
                let major = (f as i32) % 5000 == 0;
                vline(ax.x(f), if major { bright } else { fine });
                if major {
                    flabels.push((f, format!("{}k", f as i32 / 1000)));
                }
                f += 1000.0;
            }
        }
        SpecScale::St => {
            for o in 0..=10 {
                let f = 440.0 * 2f32.powf((12.0 * (o + 1) as f32 + 12.0 - 69.0) / 12.0);
                if f > LOW_HZ && f < ax.nyq {
                    vline(ax.x(f), bright);
                    flabels.push((f, format!("C{o}")));
                }
                for n in 1..12 {
                    let fn_ = f * 2f32.powf(n as f32 / 12.0);
                    if fn_ > LOW_HZ && fn_ < ax.nyq && r.width() > 600.0 {
                        vline(ax.x(fn_), fine);
                    }
                }
            }
        }
    }
    // 6 dB lines, coarser when they would crowd (≥ 14 px apart).
    let step = [6.0f32, 12.0, 24.0, 48.0].into_iter().find(|st| st * r.height() / span >= 14.0).unwrap_or(48.0);
    let mut db = (hi / step).floor() * step;
    let mut dlabels = Vec::new();
    while db > lo {
        let y = y_of(db).floor();
        if y > r.top() + 2.0 && y < r.bottom() - 2.0 {
            p.rect_filled(Rect::from_min_max(pos2(r.left(), y), pos2(r.right(), y + 1.0)), CornerRadius::ZERO, fine);
            dlabels.push((y, format!("{}", db as i32)));
        }
        db -= step;
    }
    let lc = gray(0x86);
    let lf = font(FS);
    for (y, s) in &dlabels {
        let g = p.layout_no_wrap(s.clone(), lf.clone(), lc);
        p.rect_filled(Rect::from_min_size(pos2(r.left(), y - 5.0), vec2(g.size().x + 6.0, 11.0)), CornerRadius::ZERO, gray(0x18));
        text_at(&p, r.left() + 3.0, y + 4.0, Align2::LEFT_BOTTOM, s, lf.clone(), lc);
    }
    for (f, s) in &flabels {
        let x = ax.x(*f).floor() + 0.5;
        let g = p.layout_no_wrap(s.clone(), lf.clone(), lc);
        p.rect_filled(Rect::from_center_size(pos2(x, r.top() + 6.0), vec2(g.size().x + 4.0, 11.0)), CornerRadius::ZERO, gray(0x18));
        text_at(&p, x, r.top() + 9.0, Align2::CENTER_BOTTOM, s, lf.clone(), lc);
    }

    // Curves: max hold behind the live line.
    let pts = |v: &[f32]| -> Vec<Pos2> {
        v.iter().enumerate().map(|(i, db)| pos2(r.left() + i as f32, y_of(*db).clamp(r.top() - 2.0, r.bottom() + 2.0))).collect()
    };
    let (lw, lc_live, lc_max) = if cx.on { (1.2, TEAL, MAX_COL) } else { (1.2, gray(0x69), gray(0x40)) };
    if !peak.is_empty() {
        p.add(Shape::line(pts(&peak), Stroke::new(lw, lc_max)));
    }
    if !live.is_empty() {
        p.add(Shape::line(pts(&live), Stroke::new(lw, lc_live)));
    }

    // Interaction: click resets Max, double-click unfolds / folds, the dB
    // legend drags the range (vertical scrolls, horizontal zooms).
    let legend = Rect::from_min_max(r.min, pos2(r.left() + 24.0, r.bottom()));
    let lr = ui.interact(legend, egui::Id::unique(("spec_legend", id, big)), Sense::click_and_drag());
    if lr.hovered() || lr.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if lr.drag_started() {
        cx.undo_step();
    }
    if lr.dragged() {
        let d = lr.drag_delta();
        let ppdb = r.height() / span;
        let (mut h, mut lo2) = (hi + d.y / ppdb, lo + d.y / ppdb);
        let mid = 0.5 * (h + lo2);
        let half = (0.5 * (h - lo2) * (1.0 - d.x * 0.01)).clamp(6.0, 0.5 * (SPEC_DB.1 - SPEC_DB.0));
        h = (mid + half).min(SPEC_DB.1);
        lo2 = (mid - half).max(SPEC_DB.0);
        with(cx, |s| {
            s.auto = false;
            s.hi = h;
            s.lo = lo2;
        });
    }
    let resp = ui.interact(r, egui::Id::unique(("spec_disp", id, big)), Sense::click());
    if resp.double_clicked() {
        cx.undo_step();
        with(cx, |s| s.expanded = !s.expanded);
    } else if resp.clicked() {
        cx.app.spectra.reset_peak(id);
    }
    let hover = resp.hover_pos().or(lr.hover_pos());
    if let Some(hp) = hover {
        let f = ax.f(hp.x).max(1.0);
        let fs = if f < 1000.0 { format!("{} Hz", sig3(f)) } else { format!("{} kHz", sig3(f / 1000.0)) };
        let dbv = hi - (hp.y - r.top()) / r.height() * span;
        let lines = [fs, note_name(f), format!("{dbv:.1} dB")];
        let tf = font(FS_SMALL);
        let w = lines.iter().map(|s| p.layout_no_wrap(s.clone(), tf.clone(), TEAL).size().x).fold(0.0, f32::max);
        let tb = Rect::from_min_size(pos2(r.left() + 11.0, r.bottom() - 50.0), vec2(w + 10.0, 38.0));
        p.rect_filled(tb, CornerRadius::same(4), gray(0x18));
        p.rect_stroke(tb, CornerRadius::same(4), Stroke::new(1.0, TEAL), egui::StrokeKind::Inside);
        for (j, s) in lines.iter().enumerate() {
            text_at(&p, tb.right() - 5.0, tb.top() + 12.0 + 11.0 * j as f32, Align2::RIGHT_BOTTOM, s, tf.clone(), TEAL);
        }
    }
}

/// The unfolded display above the Device View.
pub fn maximised(app: &mut DawApp, ui: &mut egui::Ui, r: Rect, c: ChainRef, id: u64) {
    let Some(dev) = chain(&app.project, c).iter().find(|d| d.id == id).cloned() else { return };
    let mut cx = Ctx::new(app, c, dev.clone());
    let p = ui.painter().clone();
    display(&mut cx, ui, &p, r, true);
    cx.commit();
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        spp::BLOCK => "Block",
        spp::CHANNEL => "Channel",
        spp::REFRESH => "Refresh",
        spp::AVG => "Avg",
        spp::LINE => "Graph",
        spp::MAX => "Max",
        spp::SCALE => "Scale X",
        spp::AUTO => "Auto",
        spp::HI => "Range Max",
        spp::LO => "Range Min",
        _ => "",
    }
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = SpectrumParams::default();
    if let FxParams::Spectrum(s) = p {
        match k {
            spp::BLOCK => s.block = d.block,
            spp::CHANNEL => s.channel = d.channel,
            spp::REFRESH => s.refresh = d.refresh,
            spp::AVG => s.avg = d.avg,
            spp::HI => s.hi = d.hi,
            spp::LO => s.lo = d.lo,
            _ => {}
        }
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    if let FxParams::Spectrum(s) = p {
        match k {
            spp::BLOCK => {
                let i = SPEC_BLOCKS.iter().position(|&b| b == s.block).unwrap_or(5) as i32 + dir.signum() as i32;
                s.block = SPEC_BLOCKS[i.clamp(0, SPEC_BLOCKS.len() as i32 - 1) as usize];
            }
            spp::AVG => s.avg = (s.avg + dir.signum()).clamp(1.0, daw_engine::spectrum::SPEC_AVG_MAX),
            spp::HI => s.hi = (s.hi + dir.signum()).clamp(s.lo + 1.0, SPEC_DB.1),
            spp::LO => s.lo = (s.lo + dir.signum()).clamp(SPEC_DB.0, s.hi - 1.0),
            _ => {
                let v = norm(&FxParams::Spectrum(*s), k).unwrap_or(0.0);
                let mut q = FxParams::Spectrum(*s);
                set_norm(&mut q, k, v + dir * 0.01);
                if let FxParams::Spectrum(n) = q {
                    *s = n;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_axis_matches_live() {
        // Live at 44.1 kHz: 100 Hz / 1 kHz / 10 kHz at 83 / 165 / 248 px of 276.
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(276.0, 162.0));
        let ax = Ax { r, scale: SpecScale::Log, nyq: 22_050.0 };
        for (f, x) in [(100.0, 83.0), (1000.0, 165.0), (10_000.0, 248.0)] {
            assert!((ax.x(f) - x).abs() < 1.5, "{f}: {}", ax.x(f));
            assert!((ax.f(ax.x(f)) - f).abs() / f < 1e-3);
        }
        let lin = Ax { r, scale: SpecScale::Lin, nyq: 22_050.0 };
        assert!((lin.x(11_025.0) - 138.0).abs() < 0.01);
        assert_eq!(width(&SpectrumParams { expanded: true, ..Default::default() }), SPEC_FOLDED_W);
    }
}
