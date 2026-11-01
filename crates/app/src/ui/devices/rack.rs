//! Audio Effect Rack device (Live 12 manual ch. 25): view column, Macro
//! Variations, Macros, Chain List (with Chain Select zones) and the selected
//! chain's devices between the rack panel and its end bracket. With Macros,
//! Variations and Chain List hidden the title folds into the view column.
//! Geometry measured from Live 12 at 100 % (l/t = rack left/top).

use super::*;
use crate::fx_ops::racks_above;
use daw_project::devices::{MacroMap, RackChain, Variation};

/// Parameter id of the Chain Selector (macros are 0..16).
pub const SELECTOR: u8 = 16;
const VIEW_W: f32 = 21.0;
const VAR_W: f32 = 141.0;
const CHAIN_PANEL_W: f32 = 244.0;
/// Chain Select zone editor column (chain list, "Chain" shown).
const ZONE_W: f32 = 136.0;
/// End bracket: 16 body + 2 border.
const END_W: f32 = 18.0;
const DROP_W: f32 = 78.0;
const SEP: f32 = 2.0;
const ROW_H: f32 = 19.0;
const SOLO_BLUE: Color32 = Color32::from_rgb(0x5a, 0xa8, 0xff);
const NAME_BG: Color32 = Color32::from_gray(0x3c);
/// Map mode: overlay on mappable parameters / on the armed macro's targets.
pub const MAP_FILL: Color32 = Color32::from_rgba_premultiplied(0x10, 0x48, 0x50, 0x60);
pub const MAP_HIT: Color32 = Color32::from_rgba_premultiplied(0x02, 0x80, 0x8e, 0x90);

fn rp(d: &Device) -> RackParams {
    match d.params {
        FxParams::Rack(p) => p,
        _ => RackParams::default(),
    }
}

fn folded(p: &RackParams) -> bool {
    !p.show_macros && !p.show_variations && !p.show_chains
}

fn mac_cols(p: &RackParams) -> usize {
    (p.visible as usize).clamp(1, MACROS).div_ceil(2)
}

/// Rack panel (unfolded) or view strip (folded) width.
fn panel_w(p: &RackParams) -> f32 {
    if folded(p) {
        return VIEW_W + if p.show_devices { 0.0 } else { 2.0 };
    }
    let mut w = VIEW_W;
    if p.show_variations {
        w += VAR_W;
    }
    if p.show_macros {
        w += mac_cols(p) as f32 * 52.0 + 4.0;
    }
    if p.show_chains {
        w += CHAIN_PANEL_W + 6.0 + if p.zones { ZONE_W } else { 0.0 };
    }
    w + 3.0
}

fn sel_chain<'a>(d: &'a Device, p: &RackParams) -> Option<&'a RackChain> {
    let r = d.rack.as_deref()?;
    r.chains.get((p.sel_chain as usize).min(r.chains.len().saturating_sub(1)))
}

pub fn width(d: &Device) -> f32 {
    let p = rp(d);
    let mut w = panel_w(&p);
    if p.show_devices {
        w += match sel_chain(d, &p) {
            Some(c) if !c.devices.is_empty() => c.devices.iter().map(|x| SEP + super::width(x)).sum::<f32>(),
            _ => SEP + DROP_W,
        };
        w += SEP + END_W;
    }
    w
}

/// A rack above chain `c` is selected (its devices draw selected).
pub fn ancestor_selected(app: &DawApp, c: ChainRef) -> bool {
    matches!(c, ChainRef::Sub(_)) && racks_above(&app.project, c).iter().any(|r| app.dev_selected(*r))
}

/// Macro name: custom, the single mapped parameter's name, else "Macro N".
fn macro_name(app: &DawApp, c: ChainRef, d: &Device, k: usize) -> String {
    let r = d.rack.as_deref();
    if let Some(n) = r.and_then(|r| r.names.get(k)).filter(|n| !n.is_empty()) {
        return n.clone();
    }
    let maps: Vec<&MacroMap> = r.map(|r| r.mappings.iter().filter(|m| m.macro_idx as usize == k).collect()).unwrap_or_default();
    if let [m] = maps.as_slice() {
        let top = crate::fx_ops::top_of(&app.project, c);
        if let Some(t) = crate::fx_ops::find_dev(&app.project, top, m.device) {
            return param_name(&t.params, m.param).to_string();
        }
    }
    format!("Macro {}", k + 1)
}

/// View-column icon: orange disc when on, grey when off, dark glyph.
fn view_icon(p: &Painter, c: Pos2, kind: u8, on: bool) {
    p.circle_filled(c, 6.5, if on { ORANGE } else { gray(0x75) });
    let g = gray(0x36);
    let s = Stroke::new(1.3, g);
    match kind {
        // Macros: knob.
        0 => {
            arc(p, c, 3.4, 0.75 * PI, 2.25 * PI, s);
            p.line_segment([c, c + vec2(-1.6, 2.4)], s);
        }
        // Variations: camera with a plus.
        1 => {
            let b = Rect::from_center_size(c + vec2(0.0, 0.5), vec2(8.0, 6.0));
            p.rect_filled(b, CornerRadius::same(1), g);
            quad(p, Rect::from_min_size(pos2(c.x - 1.5, b.top() - 1.5), vec2(3.0, 1.5)), g);
            let w = if on { ORANGE } else { gray(0x75) };
            quad(p, Rect::from_center_size(b.center(), vec2(4.0, 1.2)), w);
            quad(p, Rect::from_center_size(b.center(), vec2(1.2, 4.0)), w);
        }
        // Devices: a device strip.
        2 => {
            p.rect_stroke(Rect::from_center_size(c, vec2(9.0, 5.0)), CornerRadius::same(1), s, egui::StrokeKind::Middle);
            quad(p, Rect::from_center_size(c, vec2(7.0, 1.3)), g);
        }
        // Chain list: dotted list.
        _ => {
            for dy in [-2.5, 0.0, 2.5] {
                quad(p, Rect::from_center_size(c + vec2(-2.5, dy), vec2(1.4, 1.4)), g);
                quad(p, Rect::from_center_size(c + vec2(1.2, dy), vec2(4.6, 1.3)), g);
            }
        }
    }
}

/// Small dark round button (+ / −).
fn round_button(p: &Painter, c: Pos2, plus: bool, enabled: bool) {
    p.circle_filled(c, 6.5, gray(0x12));
    p.circle_filled(c, 5.5, gray(0x1e));
    let g = if enabled { gray(0xb5) } else { gray(0x55) };
    quad(p, Rect::from_center_size(c, vec2(6.0, 1.2)), g);
    if plus {
        quad(p, Rect::from_center_size(c, vec2(1.2, 6.0)), g);
    }
}

/// Rounded pill button (Rand / New).
fn pill(p: &Painter, r: Rect, label: &str, col: Color32) {
    p.rect_filled(r, CornerRadius::same((r.height() / 2.0) as u8), gray(0x13));
    p.rect_filled(r.shrink(1.0), CornerRadius::same((r.height() / 2.0 - 1.0) as u8), gray(0x1e));
    p.text(r.center(), Align2::CENTER_CENTER, label, font(FS), col);
}

/// Square-ish 1 px #111 button, teal when on.
fn sq_button(p: &Painter, r: Rect, label: &str, on: bool) {
    boxed(p, r, if on { TEAL } else { gray(0x1e) });
    p.text(r.center(), Align2::CENTER_CENTER, label, font(FS), if on { SEL_TEXT } else { text() });
}

fn rand_u32(seed: &mut u64) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 32) as u32
}

/// Draw rack `idx` of chain `c` in `r`.
pub fn rack(app: &mut DawApp, ui: &mut egui::Ui, r: Rect, c: ChainRef, idx: usize) {
    let Some(dev) = chain(&app.project, c).get(idx).cloned() else { return };
    let p0 = rp(&dev);
    let (l, t, b) = (r.left(), r.top(), r.bottom());
    let selected = app.dev_selected(dev.id) || ancestor_selected(app, c);
    let frame = if selected { SEL_TITLE } else { gray(0x46) };
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let fold = folded(&p0);
    let pw = panel_w(&p0);
    let pr = l + pw;

    // ---- frame -------------------------------------------------------------
    let panel_rect = Rect::from_min_max(pos2(l, t), pos2(pr, b));
    let end_here = !p0.show_devices;
    p.rect_filled(panel_rect, CornerRadius { nw: 3, sw: 3, ne: if end_here { 3 } else { 0 }, se: if end_here { 3 } else { 0 } }, frame);
    let body_r = if end_here {
        pr - 2.0
    } else if fold {
        pr
    } else {
        pr - 2.0
    };
    p.rect_filled(Rect::from_min_max(pos2(l + 2.0, t + TITLE_H), pos2(body_r, b - 2.0)), CornerRadius::ZERO, panel());

    // Body / title: select, drag, context menu (created first: widgets win).
    let body = ui.interact(panel_rect, egui::Id::unique(("rack_body", dev.id)), Sense::click());
    let title = if fold {
        Rect::from_min_max(pos2(l, t + 88.0), pos2(pr, t + 135.0))
    } else {
        Rect::from_min_max(pos2(l + 17.0, t), pos2(pr - 50.0, t + TITLE_H))
    };
    let tr = ui.interact(title, egui::Id::unique(("rack_title", dev.id)), Sense::click_and_drag());
    super::select_clicks(app, ui, c, dev.id, &body, &tr);
    tr.dnd_set_drag_payload(DragItem::ChainDevice { chain: c, id: dev.id });
    if tr.dragged() {
        super::super::cursors::set(ui.ctx(), super::super::cursors::Cur::Drag);
    }
    let (mut dup, mut del, mut ungroup, mut group) = (false, false, false, false);
    for resp in [&tr, &body] {
        menu::context(resp, 223.0, |ui| {
            menu::item(ui, "Cut", "Ctrl+X", false);
            menu::item(ui, "Copy", "Ctrl+C", false);
            dup |= menu::item(ui, "Duplicate", "Ctrl+D", true);
            del |= menu::item(ui, "Delete", "Del", true);
            menu::sep(ui);
            menu::item(ui, "Rename", "Ctrl+R", false);
            menu::item(ui, "Edit Info Text", "", false);
            menu::sep(ui);
            group |= menu::item(ui, "Group", "Ctrl+G", true);
            ungroup |= menu::item(ui, "Ungroup", "Ctrl+Shift+G", true);
            menu::sep(ui);
            menu::item(ui, "Save as Default Preset", "", false);
            menu::item(ui, "Lock to Control Surface", "", false);
        });
    }

    let mut cx = Ctx::new(app, c, dev.clone());
    // Activator.
    let ac = pos2(l + 10.5, t + 8.5);
    p.circle_filled(ac, 6.5, gray(0x12));
    p.circle_filled(ac, 5.5, if cx.on { ORANGE } else { gray(0x1e) });
    let ar = ui.interact(Rect::from_center_size(ac, vec2(13.0, 13.0)), egui::Id::unique(("dev_on", dev.id)), Sense::click());
    if ar.clicked() {
        cx.undo_step();
        cx.on = !cx.on;
    }
    let name_col = if selected { SEL_TEXT } else { text() };
    if fold {
        // Rotated title (reads bottom to top) between the icons and the buttons.
        let f = bold(FS);
        let s = fit(&p, "Audio Effect Rack", &f, 46.0);
        // The strip body stays dark when selected: light text.
        let g = p.layout_no_wrap(s, f, text());
        p.add(egui::epaint::TextShape::new(pos2(l + 3.5, t + 134.0), g, text()).with_angle(-PI / 2.0));
        for (i, dy) in [143.5, 160.5, 177.5].into_iter().enumerate() {
            header_button(&p, pos2(l + 10.5, t + dy), i);
        }
    } else {
        let nf = bold(FS);
        let right = if p0.show_macros {
            l + VIEW_W + if p0.show_variations { VAR_W } else { 0.0 } + mac_cols(&p0) as f32 * 52.0 - 2.0 - 26.0
        } else {
            pr - 52.0
        };
        let name = fit(&p, "Audio Effect Rack", &nf, right - (l + 22.0));
        text_at(&p, l + 22.0, t + 12.0, Align2::LEFT_BOTTOM, &name, nf, name_col);
        for (i, dx) in [43.5, 26.5, 9.5].into_iter().enumerate() {
            header_button(&p, pos2(pr - dx, t + 8.5), i);
        }
    }

    // ---- view column ---------------------------------------------------------
    let mut rpm = p0;
    let mut y = t + 27.5;
    let icon_hit = |ui: &egui::Ui, c0: Pos2, k: &str| {
        ui.interact(Rect::from_center_size(c0, vec2(13.0, 13.0)), egui::Id::unique(("rack_view", dev.id, k)), Sense::click())
    };
    let ic = pos2(l + 10.5, y);
    view_icon(&p, ic, 0, rpm.show_macros);
    if icon_hit(ui, ic, "m").on_hover_text("Show/Hide Macro Controls").clicked() {
        rpm.show_macros = !rpm.show_macros;
    }
    y += 17.0;
    if p0.show_macros {
        let (pc, mc) = (pos2(l + 10.5, t + 44.5), pos2(l + 10.5, t + 59.5));
        round_button(&p, pc, true, rpm.visible < MACROS as u8);
        round_button(&p, mc, false, rpm.visible > 1);
        if icon_hit(ui, pc, "+").on_hover_text("Add Macro").clicked() && rpm.visible < MACROS as u8 {
            rpm.visible += 1;
        }
        if icon_hit(ui, mc, "-").on_hover_text("Remove Macro").clicked() && rpm.visible > 1 {
            rpm.visible -= 1;
        }
        y = t + 76.5;
    }
    for (kind, tip) in [(1u8, "Show/Hide Macro Variations"), (2, "Show/Hide Devices"), (3, "Show/Hide Chain List")] {
        let ic = pos2(l + 10.5, y);
        let on = match kind {
            1 => rpm.show_variations,
            2 => rpm.show_devices,
            _ => rpm.show_chains,
        };
        view_icon(&p, ic, kind, on);
        if icon_hit(ui, ic, tip).on_hover_text(tip).clicked() {
            match kind {
                1 => rpm.show_variations = !on,
                2 => rpm.show_devices = !on,
                _ => rpm.show_chains = !on,
            }
        }
        y += 17.0;
    }

    // ---- sections --------------------------------------------------------------
    let mut x = l + VIEW_W;
    let n_map = dev.rack.as_ref().map(|r| r.mappings.len()).unwrap_or(0);
    let mut var_op: Option<(u8, usize)> = None; // 0 new, 1 launch, 2 overwrite, 3 delete, 4 duplicate
    if !fold && p0.show_variations {
        let vr = Rect::from_min_max(pos2(x, t + 38.0), pos2(x + 135.0, b - 5.0));
        boxed_col(&p, vr, panel(), gray(0x24));
        let nr = Rect::from_min_max(pos2(x + 46.0, t + 22.0), pos2(x + 89.0, t + 35.0));
        pill(&p, nr, "New", if n_map > 0 { text() } else { gray(0x75) });
        let nresp = ui.interact(nr, egui::Id::unique(("rack_new", dev.id)), Sense::click()).on_hover_text("Store a new Macro Variation");
        if nresp.clicked() {
            var_op = Some((0, 0));
        }
        let vars = dev.rack.as_ref().map(|r| r.variations.clone()).unwrap_or_default();
        if vars.is_empty() {
            p.text(vr.center(), Align2::CENTER_CENTER, "Macro Variations", font(FS), text());
        }
        let vp = p.with_clip_rect(vr.shrink(1.0).intersect(p.clip_rect()));
        for (i, v) in vars.iter().enumerate() {
            let ry = vr.top() + 1.0 + i as f32 * 17.0;
            if ry + 17.0 > vr.bottom() {
                break;
            }
            let row = Rect::from_min_max(pos2(vr.left() + 1.0, ry), pos2(vr.right() - 1.0, ry + 16.0));
            let ow = Rect::from_min_size(pos2(row.left() + 2.0, ry + 2.0), vec2(13.0, 12.0));
            let la = Rect::from_min_size(pos2(row.right() - 15.0, ry + 2.0), vec2(13.0, 12.0));
            boxed(&vp, ow, gray(0x1e));
            vp.rect_filled(Rect::from_center_size(ow.center(), vec2(6.0, 6.0)), CornerRadius::same(1), gray(0xb5));
            boxed(&vp, la, gray(0x1e));
            let lc = la.center();
            vp.add(Shape::convex_polygon(vec![lc + vec2(-2.5, -3.5), lc + vec2(3.5, 0.0), lc + vec2(-2.5, 3.5)], gray(0xb5), Stroke::NONE));
            let fs = font(FS);
            let name = fit(&vp, &v.name, &fs, la.left() - ow.right() - 8.0);
            text_at(&vp, ow.right() + 5.0, ry + 12.0, Align2::LEFT_BOTTOM, &name, fs, text());
            quad(&vp, Rect::from_min_size(pos2(row.left(), ry + 16.0), vec2(row.width(), 1.0)), gray(0x24));
            let rr = ui.interact(row, egui::Id::unique(("rack_var", dev.id, i)), Sense::click());
            if ui.interact(ow, egui::Id::unique(("rack_var_ow", dev.id, i)), Sense::click()).on_hover_text("Overwrite").clicked() {
                var_op = Some((2, i));
            }
            if ui.interact(la, egui::Id::unique(("rack_var_la", dev.id, i)), Sense::click()).on_hover_text("Launch").clicked()
                || rr.double_clicked()
            {
                var_op = Some((1, i));
            }
            menu::context(&rr, 180.0, |ui| {
                menu::item(ui, "Rename", "Ctrl+R", false);
                if menu::item(ui, "Duplicate", "Ctrl+D", true) {
                    var_op = Some((4, i));
                }
                if menu::item(ui, "Delete", "Del", true) {
                    var_op = Some((3, i));
                }
            });
        }
        x += VAR_W;
    }
    let mut rand = false;
    if !fold && p0.show_macros {
        let cols = mac_cols(&p0);
        let mr = x + cols as f32 * 52.0 - 2.0;
        // Rand / Map in the title bar.
        let rr = Rect::from_min_max(pos2(mr - 24.0, t + 2.0), pos2(mr + 7.0, t + 15.0));
        pill(&p, rr, "Rand", text());
        if ui.interact(rr, egui::Id::unique(("rack_rand", dev.id)), Sense::click()).on_hover_text("Randomize the mapped Macros").clicked() {
            rand = true;
        }
        let mpr = Rect::from_min_max(pos2(mr + 11.0, t + 2.0), pos2(mr + 36.0, t + 15.0));
        sq_button(&p, mpr, "Map", rpm.map);
        if ui.interact(mpr, egui::Id::unique(("rack_map", dev.id)), Sense::click()).on_hover_text("Macro Map Mode").clicked() {
            rpm.map = !rpm.map;
            rpm.map_sel = None;
        }
        for k in 0..(p0.visible as usize).min(MACROS) {
            let (col, row) = (k % cols, k / cols);
            let bx = Rect::from_min_size(pos2(x + col as f32 * 52.0, t + 21.0 + row as f32 * 82.0), vec2(50.0, 80.0));
            boxed_col(&p, bx, panel(), gray(0x24));
            quad(&p, Rect::from_min_max(bx.min + vec2(1.0, 1.0), pos2(bx.right() - 1.0, bx.top() + 29.0)), gray(0x46));
            let fs = font(FS);
            let nm = fit(&p, &macro_name(cx.app, c, &dev, k), &fs, 46.0);
            text_at(&p, bx.center().x, bx.top() + 12.5, Align2::CENTER_BOTTOM, &nm, fs.clone(), text());
            let kc = pos2(bx.left() + 25.5, bx.top() + 48.5);
            let v = rpm.macros[k];
            // Unmapped macros draw like disabled knobs (Live).
            let mapped = dev.rack.as_ref().is_some_and(|r| r.mappings.iter().any(|m| m.macro_idx as usize == k));
            knob_look_m(&p, kc, v / 127.0, 0.0, mapped, false);
            cx.drag(ui, Rect::from_center_size(kc, vec2(30.0, 30.0)), k as u8, true);
            if let FxParams::Rack(q) = cx.params {
                rpm.macros[k] = q.macros[k];
            }
            if rpm.map {
                let mb = Rect::from_min_max(pos2(bx.left() + 9.0, bx.top() + 63.0), pos2(bx.right() - 9.0, bx.top() + 76.0));
                sq_button(&p, mb, "Map", rpm.map_sel == Some(k as u8));
                if ui.interact(mb, egui::Id::unique(("rack_mapk", dev.id, k)), Sense::click()).clicked() {
                    rpm.map_sel = if rpm.map_sel == Some(k as u8) { None } else { Some(k as u8) };
                }
            } else {
                text_at(&p, bx.center().x, bx.top() + 70.5, Align2::CENTER_BOTTOM, &format!("{:.0}", v), fs, text());
            }
        }
        x += cols as f32 * 52.0 + 4.0;
    }
    let chains_x = x;
    // Chain list header toggles (rack params).
    if !fold && p0.show_chains {
        // Chain / Hide stay above the chain rows (the zone ruler is right of them).
        let cr = chains_x + CHAIN_PANEL_W;
        let au = Rect::from_min_max(pos2(chains_x + 3.0, t + 27.0), pos2(chains_x + 20.0, t + 40.0));
        boxed(&p, au, if rpm.auto_select { TEAL } else { gray(0x1e) });
        let gc = au.center();
        let g = if rpm.auto_select { SEL_TEXT } else { gray(0xb5) };
        quad(&p, Rect::from_min_size(gc + vec2(-4.0, -2.0), vec2(1.0, 4.0)), g);
        quad(&p, Rect::from_min_size(gc + vec2(-2.0, -2.0), vec2(1.0, 4.0)), g);
        p.add(Shape::convex_polygon(vec![gc + vec2(0.0, -2.5), gc + vec2(3.0, 0.0), gc + vec2(0.0, 2.5)], g, Stroke::NONE));
        if ui.interact(au, egui::Id::unique(("rack_auto", dev.id)), Sense::click()).on_hover_text("Auto Select").clicked() {
            rpm.auto_select = !rpm.auto_select;
        }
        let chb = Rect::from_min_max(pos2(cr - 71.0, t + 27.0), pos2(cr - 41.0, t + 40.0));
        let hib = Rect::from_min_max(pos2(cr - 38.0, t + 27.0), pos2(cr - 8.0, t + 40.0));
        sq_button(&p, chb, "Chain", rpm.zones);
        sq_button(&p, hib, "Hide", !rpm.zones);
        if ui.interact(chb, egui::Id::unique(("rack_zc", dev.id)), Sense::click()).on_hover_text("Show the Chain Select Editor").clicked() {
            rpm.zones = true;
        }
        if ui.interact(hib, egui::Id::unique(("rack_zh", dev.id)), Sense::click()).on_hover_text("Hide the Zone Editor").clicked() {
            rpm.zones = false;
        }
    }
    // Merge the view / macro edits into the device params.
    if let FxParams::Rack(q) = &mut cx.params {
        let macros = q.macros;
        let changed_view = (q.show_macros, q.show_variations, q.show_chains, q.show_devices, q.visible, q.zones, q.auto_select)
            != (rpm.show_macros, rpm.show_variations, rpm.show_chains, rpm.show_devices, rpm.visible, rpm.zones, rpm.auto_select);
        *q = rpm;
        q.macros = macros;
        if changed_view {
            cx.undo_step();
        }
    }
    let app = cx.commit();

    // ---- Rand / variations (structural or macro writes) ----------------------
    if rand {
        let mut q = rp(chain(&app.project, c).get(idx).unwrap_or(&dev));
        let maps = dev.rack.as_ref().map(|r| r.mappings.clone()).unwrap_or_default();
        let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
        let mut any = false;
        for k in 0..MACROS {
            if q.no_rand & (1 << k) == 0 && maps.iter().any(|m| m.macro_idx as usize == k) {
                q.macros[k] = (rand_u32(&mut seed) % 128) as f32;
                any = true;
            }
        }
        if any {
            app.begin_gesture();
            app.set_device(c, dev.id, dev.on, FxParams::Rack(q));
        }
    }
    if let Some((op, i)) = var_op {
        let cur = rp(chain(&app.project, c).get(idx).unwrap_or(&dev));
        match op {
            1 => {
                let vals = dev.rack.as_ref().and_then(|r| r.variations.get(i)).map(|v| v.values);
                if let Some(vals) = vals {
                    let mut q = cur;
                    for (k, v) in vals.iter().enumerate() {
                        if q.no_var & (1 << k) == 0 {
                            q.macros[k] = *v;
                        }
                    }
                    app.begin_gesture();
                    app.set_device(c, dev.id, dev.on, FxParams::Rack(q));
                }
            }
            _ => {
                app.edit_rack(c, dev.id, |p, _| {
                    let Some(r) = crate::fx_ops::chain_mut(p, c)
                        .and_then(|v| v.iter_mut().find(|d| d.id == dev.id))
                        .and_then(|d| d.rack.as_deref_mut())
                    else {
                        return;
                    };
                    match op {
                        0 => {
                            let n = r.variations.len() + 1;
                            r.variations.push(Variation { name: format!("Variation {n}"), values: cur.macros });
                        }
                        2 => {
                            if let Some(v) = r.variations.get_mut(i) {
                                v.values = cur.macros;
                            }
                        }
                        3 => {
                            if i < r.variations.len() {
                                r.variations.remove(i);
                            }
                        }
                        _ => {
                            if let Some(v) = r.variations.get(i).cloned() {
                                r.variations.insert(i + 1, Variation { name: format!("{} copy", v.name), ..v });
                            }
                        }
                    }
                });
            }
        }
    }

    // ---- chain list --------------------------------------------------------------
    let dev = chain(&app.project, c).get(idx).cloned().unwrap_or(dev);
    let p1 = rp(&dev);
    let chains: Vec<RackChain> = dev.rack.as_ref().map(|r| r.chains.clone()).unwrap_or_default();
    let cur_sel = (p1.sel_chain as usize).min(chains.len().saturating_sub(1));
    if !fold && p1.show_chains {
        chain_list(app, ui, &p, c, &dev, &p1, &chains, cur_sel, chains_x, t, b);
    }

    // ---- devices of the selected chain ------------------------------------------
    if p1.show_devices {
        let mut x = pr;
        let bot = b - 2.0;
        let sep = |p: &Painter, x: f32| quad(p, Rect::from_min_max(pos2(x, t), pos2(x + SEP, bot)), gray(0x24));
        let mut slots = Vec::new();
        let sub = chains.get(cur_sel).map(|ch| ChainRef::Sub(ch.id));
        let devs = chains.get(cur_sel).map(|ch| ch.devices.clone()).unwrap_or_default();
        let area_l = x;
        for (i, d) in devs.iter().enumerate() {
            sep(&p, x);
            let w = super::width(d);
            let dr = Rect::from_min_max(pos2(x + SEP, t), pos2(x + SEP + w, bot));
            if let Some(sc) = sub {
                super::device(app, ui, dr, sc, i);
            }
            slots.push((dr.left(), dr.right()));
            x = dr.right();
        }
        if devs.is_empty() {
            sep(&p, x);
            let dz = Rect::from_min_max(pos2(x + SEP, t), pos2(x + SEP + DROP_W, bot));
            p.rect_filled(dz, CornerRadius::ZERO, panel());
            for (dy, s) in [(-7.5, "Drop Audio"), (7.5, "Effects Here")] {
                p.text(dz.center() + vec2(0.0, dy), Align2::CENTER_CENTER, s, font(FS), text());
            }
            x = dz.right();
        }
        sep(&p, x);
        x += SEP;
        // Bottom border under the devices, then the end bracket.
        quad(&p, Rect::from_min_max(pos2(pr, bot), pos2(x, b)), frame);
        let er = Rect::from_min_max(pos2(x, t), pos2(x + END_W, b));
        p.rect_filled(er, CornerRadius { nw: 0, sw: 0, ne: 3, se: 3 }, frame);
        quad(&p, Rect::from_min_max(pos2(x, t + TITLE_H), pos2(x + END_W - 2.0, bot)), panel());
        // Drops onto the devices area: into the selected chain (a new chain
        // if the rack has none).
        let area = Rect::from_min_max(pos2(area_l, t), pos2(x, b));
        let zone = ui.interact(area, egui::Id::unique(("rack_drop", dev.id)), Sense::hover());
        drop_devices(app, ui, &zone, c, &dev, sub, &slots, area_l + 1.0);
    }

    // ---- pending map request (a parameter clicked in Map mode) ------------------
    take_map_request(app, ui.ctx(), c, &dev);
    // Menu commands act on the whole selection when the device is in it.
    let in_sel = app.dev_selected(dev.id);
    if dup {
        if in_sel {
            app.duplicate_selected_device();
        } else {
            app.duplicate_device(c, dev.id);
        }
    }
    if group {
        if in_sel {
            app.group_selected_device(false);
        } else {
            app.group_device(c, dev.id);
        }
    }
    if ungroup {
        if in_sel {
            app.group_selected_device(true);
        } else {
            app.ungroup_device(c, dev.id);
        }
    }
    if del {
        match app.selected_devices() {
            Some((sc, ids)) if in_sel && ids.len() > 1 => app.delete_devices(sc, &ids),
            _ => app.delete_device(c, dev.id),
        }
    }
}

/// 1 px `edge` outline box with `fill`.
fn boxed_col(p: &Painter, r: Rect, fill: Color32, edge: Color32) {
    quad(p, r, edge);
    quad(p, r.shrink(1.0), fill);
}

/// Hot-swap / save / menu header button `i` (greyed, not implemented).
pub fn header_button(p: &Painter, bc: Pos2, i: usize) {
    let glyph = gray(0x75);
    p.circle_filled(bc, 6.5, gray(0x1e));
    match i {
        0 => {
            arc(p, bc, 3.2, -2.6, -0.3, Stroke::new(1.2, glyph));
            arc(p, bc, 3.2, 0.55, 2.85, Stroke::new(1.2, glyph));
            p.add(Shape::convex_polygon(vec![bc + vec2(1.2, -4.8), bc + vec2(3.8, -2.4), bc + vec2(0.6, -1.6)], glyph, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![bc + vec2(-1.2, 4.8), bc + vec2(-3.8, 2.4), bc + vec2(-0.6, 1.6)], glyph, Stroke::NONE));
        }
        1 => {
            let f = Rect::from_center_size(bc, vec2(7.0, 7.0));
            p.rect_filled(f, CornerRadius::same(1), glyph);
            p.rect_filled(Rect::from_min_size(f.min + vec2(2.0, 1.0), vec2(3.0, 2.0)), CornerRadius::ZERO, gray(0x1e));
        }
        _ => {
            for k in [-3.0, 0.0, 3.0] {
                p.circle_filled(bc + vec2(k, 0.0), 0.9, glyph);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn chain_list(
    app: &mut DawApp,
    ui: &mut egui::Ui,
    p: &Painter,
    c: ChainRef,
    dev: &Device,
    rpm: &RackParams,
    chains: &[RackChain],
    cur: usize,
    x: f32,
    t: f32,
    b: f32,
) {
    let zw = if rpm.zones { ZONE_W } else { 0.0 };
    let panel_r = Rect::from_min_max(pos2(x, t + 47.0), pos2(x + CHAIN_PANEL_W + zw, b - 5.0));
    boxed_col(p, panel_r, panel(), gray(0x24));
    let cp = p.with_clip_rect(panel_r.shrink(1.0).intersect(p.clip_rect()));
    let mut select: Option<usize> = None;
    let mut del: Option<usize> = None;
    let mut dupc: Option<usize> = None;
    let sel_v = rpm.selector.round().clamp(0.0, 127.0) as u8;
    for (i, ch) in chains.iter().enumerate() {
        let y = panel_r.top() + 1.0 + i as f32 * ROW_H;
        if y + 18.0 > panel_r.bottom() {
            break;
        }
        let mut mix = ch.mix;
        let name_r = Rect::from_min_max(pos2(x + 11.0, y), pos2(x + 87.0, y + 18.0));
        quad(&cp, name_r, if i == cur { NAME_BG } else { panel() });
        let fs = bold(FS);
        let nm = fit(&cp, &ch.label(), &fs, 70.0);
        text_at(&cp, x + 14.0, y + 13.0, Align2::LEFT_BOTTOM, &nm, fs, text());
        for gx in [x + 89.0, x + 92.0, x + 237.0 + zw, x + 240.0 + zw] {
            quad(&cp, Rect::from_min_max(pos2(gx, y + 3.0), pos2(gx + 2.0, y + 16.0)), gray(0x18));
        }
        let row_r = Rect::from_min_max(pos2(x + 1.0, y), pos2(x + 87.0, y + 18.0));
        let nresp = ui.interact(row_r, egui::Id::unique(("rack_chain", dev.id, ch.id)), Sense::click());
        if nresp.clicked() || nresp.secondary_clicked() {
            select = Some(i);
        }
        menu::context(&nresp, 200.0, |ui| {
            menu::item(ui, "Cut", "Ctrl+X", false);
            menu::item(ui, "Copy", "Ctrl+C", false);
            if menu::item(ui, "Duplicate", "Ctrl+D", true) {
                dupc = Some(i);
            }
            if menu::item(ui, "Delete", "Del", true) {
                del = Some(i);
            }
            menu::sep(ui);
            menu::item(ui, "Rename", "Ctrl+R", false);
            menu::item(ui, "Edit Info Text", "", false);
            menu::sep(ui);
            menu::item(ui, "Save Chain as Preset", "", false);
        });
        // Volume / pan.
        let vr = Rect::from_min_max(pos2(x + 97.0, y + 2.0), pos2(x + 135.0, y + 15.0));
        let mut db = if mix.volume <= CHAIN_VOL_MIN { crate::ui::METER_FLOOR_DB } else { mix.volume };
        let (vresp, vch) = super::super::widgets::chain_volume_box(ui, vr, egui::Id::unique(("rack_vol", ch.id)), &mut db);
        if vresp.drag_started() || vresp.double_clicked() {
            app.begin_gesture();
        }
        if vch {
            mix.volume = if db <= crate::ui::METER_FLOOR_DB { CHAIN_VOL_MIN - 1.0 } else { db.min(CHAIN_VOL_MAX) };
        }
        let pr = Rect::from_min_max(pos2(x + 139.0, y + 2.0), pos2(x + 177.0, y + 15.0));
        let mut pan = mix.pan;
        let (presp, pch) = super::super::widgets::pan_box_ex(ui, pr, false, egui::Id::unique(("rack_pan", ch.id)), &mut pan);
        if presp.drag_started() || presp.double_clicked() {
            app.begin_gesture();
        }
        if pch {
            mix.pan = pan;
        }
        // Activator (speaker) and Solo.
        let ar = Rect::from_min_max(pos2(x + 181.0, y + 2.0), pos2(x + 197.0, y + 15.0));
        boxed(&cp, ar, if mix.on { ORANGE } else { gray(0x1e) });
        let sc = ar.center();
        let g = if mix.on { gray(0x07) } else { gray(0xb5) };
        // Speaker outline.
        let st = Stroke::new(1.0, g);
        cp.rect_stroke(Rect::from_min_size(sc + vec2(-3.5, -1.5), vec2(2.5, 3.0)), CornerRadius::ZERO, st, egui::StrokeKind::Middle);
        cp.add(Shape::closed_line(vec![sc + vec2(-1.0, -1.5), sc + vec2(2.0, -4.0), sc + vec2(2.0, 4.0), sc + vec2(-1.0, 1.5)], st));
        if ui.interact(ar, egui::Id::unique(("rack_on", ch.id)), Sense::click()).on_hover_text("Chain Activator").clicked() {
            app.begin_gesture();
            mix.on = !mix.on;
        }
        let sr = Rect::from_min_max(pos2(x + 201.0, y + 2.0), pos2(x + 217.0, y + 15.0));
        boxed(&cp, sr, if mix.solo { SOLO_BLUE } else { gray(0x1e) });
        cp.text(sr.center(), Align2::CENTER_CENTER, "S", font(FS), if mix.solo { SEL_TEXT } else { text() });
        if ui.interact(sr, egui::Id::unique(("rack_solo", ch.id)), Sense::click()).on_hover_text("Solo").clicked() {
            app.begin_gesture();
            mix.solo = !mix.solo;
        }
        header_button(&cp, pos2(x + 227.5, y + 8.5), 0);
        // Chain Select zone.
        if rpm.zones {
            let zr = Rect::from_min_max(pos2(x + 244.0, y + 3.0), pos2(x + 244.0 + zw - 6.0, y + 15.0));
            zone_bar(app, ui, &cp, zr, ch.id, &mut mix.zone);
        }
        quad(&cp, Rect::from_min_size(pos2(x + 1.0, y + 18.0), vec2(panel_r.width() - 2.0, 1.0)), gray(0x24));
        if mix != ch.mix {
            app.set_chain_mix(ch.id, mix);
        }
    }
    // Zone editor ruler: the Chain Selector.
    if rpm.zones {
        let zr = Rect::from_min_max(pos2(x + 244.0, t + 29.0), pos2(x + 244.0 + zw - 6.0, t + 40.0));
        boxed(p, zr, gray(0x1e));
        let fx = |v: f32| zr.left() + 1.0 + (zr.width() - 2.0) * v / 127.0;
        let sx = fx(sel_v as f32);
        quad(p, Rect::from_min_max(pos2(sx - 1.0, zr.top() + 1.0), pos2(sx + 1.0, zr.bottom() - 1.0)), ORANGE);
        p.text(pos2(zr.right() - 3.0, zr.center().y), Align2::RIGHT_CENTER, format!("{sel_v}"), font(FS_SMALL), text());
        let resp = ui.interact(zr, egui::Id::unique(("rack_selr", dev.id)), Sense::click_and_drag()).on_hover_text("Chain Selector");
        if let Some(pt) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
            if resp.drag_started() || resp.clicked() {
                app.begin_gesture();
            }
            let v = ((pt.x - zr.left() - 1.0) / (zr.width() - 2.0) * 127.0).round().clamp(0.0, 127.0);
            let mut q = *rpm;
            q.selector = v;
            if q.auto_select
                && let Some(i) = chains.iter().position(|ch| ch.mix.zone.contains(v as u8))
            {
                q.sel_chain = i as u16;
            }
            app.set_device(c, dev.id, dev.on, FxParams::Rack(q));
        }
        // Selector line through the zones.
        let line_x = sx;
        quad(p, Rect::from_min_max(pos2(line_x - 0.5, panel_r.top() + 1.0), pos2(line_x + 0.5, panel_r.bottom() - 1.0)), gray(0x75));
    }
    // Drop area below the chains: dropped devices make new chains.
    let rows_h = chains.len() as f32 * ROW_H;
    let da = Rect::from_min_max(pos2(panel_r.left() + 1.0, panel_r.top() + 1.0 + rows_h), panel_r.max - vec2(1.0, 1.0));
    if da.height() > 12.0 {
        cp.text(da.center(), Align2::CENTER_CENTER, "Drop Audio Effects Here", font(FS), text());
        let dz = ui.interact(da, egui::Id::unique(("rack_cdrop", dev.id)), Sense::click());
        let mut create = false;
        menu::context(&dz, 180.0, |ui| {
            create = menu::item(ui, "Create Chain", "", true);
        });
        if dz.clicked() {
            app.dev_sel = Some(dev.id);
            app.dev_focus = None;
        }
        if create {
            app.add_chain(c, dev.id, None);
        }
        if let Some(h) = dz.dnd_hover_payload::<DragItem>()
            && matches!(&*h, DragItem::AudioEffect(_) | DragItem::ChainDevice { .. })
        {
            app.dev_drop_claimed = true;
            p.rect_stroke(da.shrink(1.0), CornerRadius::ZERO, Stroke::new(1.0, focus()), egui::StrokeKind::Inside);
        }
        if let Some(item) = dz.dnd_release_payload::<DragItem>() {
            app.dev_drop_claimed = true;
            match &*item {
                DragItem::AudioEffect(fx) => app.add_chain(c, dev.id, Some((None, *fx))),
                DragItem::ChainDevice { chain: from, id } => {
                    let copy = ui.input(|i| i.modifiers.command);
                    app.add_chain(c, dev.id, Some((Some((*from, *id, copy)), FxParams::Rack(Default::default()))));
                }
                _ => {}
            }
        }
    }
    if let Some(i) = select {
        let mut q = *rpm;
        q.sel_chain = i as u16;
        app.dev_sel = Some(dev.id);
        app.dev_focus = None;
        app.focus_detail = true;
        app.set_device(c, dev.id, dev.on, FxParams::Rack(q));
    }
    if let Some(i) = dupc {
        app.edit_rack(c, dev.id, |p, alloc| {
            let Some(r) =
                crate::fx_ops::chain_mut(p, c).and_then(|v| v.iter_mut().find(|d| d.id == dev.id)).and_then(|d| d.rack.as_deref_mut())
            else {
                return;
            };
            if let Some(mut ch) = r.chains.get(i).cloned() {
                ch.id = alloc();
                for d in &mut ch.devices {
                    d.reid(alloc);
                }
                r.chains.insert(i + 1, ch);
            }
        });
    }
    if let Some(i) = del {
        app.edit_rack(c, dev.id, |p, _| {
            if let Some(d) = crate::fx_ops::chain_mut(p, c).and_then(|v| v.iter_mut().find(|d| d.id == dev.id))
                && let Some(r) = d.rack.as_deref_mut()
                && i < r.chains.len()
            {
                let gone = r.chains.remove(i);
                let mut ids = Vec::new();
                for x in &gone.devices {
                    x.ids(&mut ids);
                }
                r.mappings.retain(|m| !ids.contains(&m.device));
                if let FxParams::Rack(q) = &mut d.params {
                    q.sel_chain = q.sel_chain.min(r.chains.len().saturating_sub(1) as u16);
                }
            }
        });
    }
}

/// One chain's zone bar: drag the body to move, the ends to resize; Shift
/// on an end drags its fade.
fn zone_bar(app: &mut DawApp, ui: &mut egui::Ui, p: &Painter, r: Rect, id: u64, z: &mut Zone) {
    let fx = |v: f32| r.left() + r.width() * v / 128.0;
    let zz = z.normalized();
    let (x0, x1) = (fx(zz.lo as f32), fx(zz.hi as f32 + 1.0));
    quad(p, r, gray(0x24));
    let bar = Rect::from_min_max(pos2(x0, r.top()), pos2(x1.max(x0 + 1.0), r.bottom()));
    quad(p, bar, gray(0x75));
    // Fades: darker wedges.
    let (f0, f1) = (fx(zz.fade_lo as f32), fx(zz.fade_hi as f32 + 1.0));
    if f0 > x0 {
        quad(p, Rect::from_min_max(bar.min, pos2(f0, bar.center().y)), gray(0x55));
    }
    if f1 < x1 {
        quad(p, Rect::from_min_max(pos2(f1, bar.top()), pos2(x1, bar.center().y)), gray(0x55));
    }
    let resp = ui.interact(r, egui::Id::unique(("rack_zone", id)), Sense::click_and_drag());
    let mem = egui::Id::unique(("rack_zone_drag", id));
    if resp.drag_started()
        && let Some(pt) = resp.interact_pointer_pos()
    {
        app.begin_gesture();
        let mode: u8 = if (pt.x - x0).abs() < 4.0 {
            1
        } else if (pt.x - x1).abs() < 4.0 {
            2
        } else {
            0
        };
        ui.data_mut(|d| d.insert_temp(mem, (mode, pt.x, *z)));
    }
    if resp.dragged()
        && let (Some(pt), Some((mode, ax, z0))) = (resp.interact_pointer_pos(), ui.data(|d| d.get_temp::<(u8, f32, Zone)>(mem)))
    {
        let dv = ((pt.x - ax) / r.width() * 128.0).round() as i32;
        let cl = |v: i32| v.clamp(0, 127) as u8;
        let fade = ui.input(|i| i.modifiers.shift);
        let mut n = z0;
        match (mode, fade) {
            (1, false) => {
                n.lo = cl(z0.lo as i32 + dv).min(z0.hi);
                n.fade_lo = n.fade_lo.max(n.lo);
            }
            (2, false) => {
                n.hi = cl(z0.hi as i32 + dv).max(z0.lo);
                n.fade_hi = n.fade_hi.min(n.hi);
            }
            (1, true) => n.fade_lo = cl(z0.fade_lo as i32 + dv),
            (2, true) => n.fade_hi = cl(z0.fade_hi as i32 + dv),
            _ => {
                let d = dv.clamp(-(z0.lo as i32), 127 - z0.hi as i32);
                n.lo = cl(z0.lo as i32 + d);
                n.hi = cl(z0.hi as i32 + d);
                n.fade_lo = cl(z0.fade_lo as i32 + d);
                n.fade_hi = cl(z0.fade_hi as i32 + d);
            }
        }
        *z = n.normalized();
    }
    if resp.hovered() {
        let near = resp.hover_pos().is_some_and(|pt| (pt.x - x0).abs() < 4.0 || (pt.x - x1).abs() < 4.0);
        if near {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
}

/// Drag & drop onto the rack's devices area.
#[allow(clippy::too_many_arguments)]
fn drop_devices(
    app: &mut DawApp,
    ui: &egui::Ui,
    zone: &Response,
    c: ChainRef,
    dev: &Device,
    sub: Option<ChainRef>,
    slots: &[(f32, f32)],
    x0: f32,
) {
    let Some(h) = zone.dnd_hover_payload::<DragItem>() else { return };
    if !matches!(&*h, DragItem::AudioEffect(_) | DragItem::ChainDevice { .. }) {
        return;
    }
    // Hovering the rack itself (dragging the rack's own title) is a no-op.
    if let DragItem::ChainDevice { id, .. } = &*h
        && *id == dev.id
    {
        return;
    }
    app.dev_drop_claimed = true;
    let ptr = ui.ctx().pointer_latest_pos().unwrap_or(zone.rect.center());
    let at = slots.iter().filter(|s| (s.0 + s.1) / 2.0 < ptr.x).count();
    let lx = if slots.is_empty() {
        x0 + 1.0
    } else if at == 0 {
        slots[0].0 - 1.0
    } else {
        slots[at - 1].1 + 1.0
    };
    ui.painter().rect_filled(
        Rect::from_min_max(pos2(lx - 1.0, zone.rect.top()), pos2(lx + 1.0, zone.rect.bottom())),
        CornerRadius::ZERO,
        focus(),
    );
    let Some(item) = zone.dnd_release_payload::<DragItem>() else { return };
    let copy = ui.input(|i| i.modifiers.command);
    match (&*item, sub) {
        (DragItem::AudioEffect(fx), Some(sc)) => app.insert_device(sc, Some(at), *fx),
        (DragItem::ChainDevice { chain: from, id }, Some(sc)) => app.move_device(*from, *id, sc, at, copy),
        (DragItem::AudioEffect(fx), None) => app.add_chain(c, dev.id, Some((None, *fx))),
        (DragItem::ChainDevice { chain: from, id }, None) => {
            app.add_chain(c, dev.id, Some((Some((*from, *id, copy)), FxParams::Rack(Default::default()))))
        }
        _ => {}
    }
}

// ----------------------------------------------------------------------------
// Map mode.
// ----------------------------------------------------------------------------

/// Map mode state for a device in chain `c`: the innermost enclosing rack in
/// Map mode (rack chain, rack id, armed macro).
pub fn map_ctx(app: &DawApp, c: ChainRef) -> Option<(ChainRef, u64, Option<u8>)> {
    if !matches!(c, ChainRef::Sub(_)) {
        return None;
    }
    let top = crate::fx_ops::top_of(&app.project, c);
    for rid in racks_above(&app.project, c) {
        let rc = crate::fx_ops::chain_of(&app.project, top, rid)?;
        let d = crate::fx_ops::find_dev(&app.project, top, rid)?;
        if let FxParams::Rack(q) = d.params
            && q.map
        {
            return Some((rc, rid, q.map_sel));
        }
    }
    None
}

fn req_id() -> egui::Id {
    egui::Id::unique("rack_map_request")
}

/// A parameter was clicked in Map mode: (rack id, macro, device id, param).
pub fn request_map(ctx: &egui::Context, rack: u64, mac: u8, dev: u64, k: u8) {
    ctx.data_mut(|d| d.insert_temp(req_id(), (rack, mac, dev, k)));
}

/// Map parameter `k` of device `target` to macro `mac` of rack `rack` (in
/// chain `rc`); the macro takes the parameter's current value.
pub fn add_mapping(app: &mut DawApp, rc: ChainRef, rack: u64, mac: u8, target: u64, k: u8) {
    let top = crate::fx_ops::top_of(&app.project, rc);
    let Some(t) = crate::fx_ops::find_dev(&app.project, top, target) else { return };
    let v = norm(&t.params, k).unwrap_or(0.0);
    app.edit_rack(rc, rack, |p, _| {
        let Some(d) = crate::fx_ops::chain_mut(p, rc).and_then(|v| v.iter_mut().find(|d| d.id == rack)) else { return };
        if let Some(r) = d.rack.as_deref_mut() {
            r.mappings.retain(|m| !(m.device == target && m.param == k));
            r.mappings.push(MacroMap { macro_idx: mac, device: target, param: k, min: 0.0, max: 1.0 });
        }
        if let FxParams::Rack(q) = &mut d.params {
            q.macros[mac as usize] = (v * 127.0).round();
        }
    });
}

/// Remove / invert the mapping of parameter `k` of device `target`.
pub fn edit_mapping(app: &mut DawApp, c: ChainRef, target: u64, k: u8, invert: bool) {
    let Some((rack, _)) = app.mapped(c, target, k) else { return };
    let top = crate::fx_ops::top_of(&app.project, c);
    let Some(rc) = crate::fx_ops::chain_of(&app.project, top, rack) else { return };
    app.edit_rack(rc, rack, |p, _| {
        let Some(r) = crate::fx_ops::chain_mut(p, rc).and_then(|v| v.iter_mut().find(|d| d.id == rack)).and_then(|d| d.rack.as_deref_mut())
        else {
            return;
        };
        if invert {
            for m in r.mappings.iter_mut().filter(|m| m.device == target && m.param == k) {
                std::mem::swap(&mut m.min, &mut m.max);
            }
        } else {
            r.mappings.retain(|m| !(m.device == target && m.param == k));
        }
    });
    if invert && let Some(i) = Some(app.mapped(c, target, k).map(|x| x.1 as usize)).flatten() {
        app.apply_macros(rc, rack, &[i]);
    }
}

/// First macro of the innermost rack above `c` without mappings.
pub fn free_macro(app: &DawApp, c: ChainRef) -> Option<(ChainRef, u64, u8)> {
    let top = crate::fx_ops::top_of(&app.project, c);
    let rid = *racks_above(&app.project, c).first()?;
    let rc = crate::fx_ops::chain_of(&app.project, top, rid)?;
    let d = crate::fx_ops::find_dev(&app.project, top, rid)?;
    let r = d.rack.as_deref()?;
    let vis = rp(d).visible as usize;
    (0..vis.max(1)).find(|k| !r.mappings.iter().any(|m| m.macro_idx as usize == *k)).map(|k| (rc, rid, k as u8))
}

fn take_map_request(app: &mut DawApp, ctx: &egui::Context, c: ChainRef, dev: &Device) {
    let Some((rack, mac, target, k)) = ctx.data(|d| d.get_temp::<(u64, u8, u64, u8)>(req_id())) else { return };
    if rack != dev.id {
        return;
    }
    ctx.data_mut(|d| d.remove::<(u64, u8, u64, u8)>(req_id()));
    add_mapping(app, c, rack, mac, target, k);
}
