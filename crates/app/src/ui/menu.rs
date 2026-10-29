//! Live 12 context menus, measured from screenshots (1.0 scale):
//! #242424 fill, 1 px #b5b5b5 border and separators, 15 px rows, labels 16 px
//! from the border, shortcuts right-aligned 6 px from it, check marks at 5 px.
//! Spacing: 1 px under the top border, 5 px above / 4 px below a separator.

use super::theme::{FS, font, gray};
use eframe::egui::{self, Align2, Color32, CornerRadius, Frame, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

/// Menu widths (outer, border included).
pub const W_CLIP: f32 = 273.0;
pub const W_TRACK: f32 = 252.0;

const ROW: f32 = 15.0;
const TEXT_X: f32 = 15.0; // from the content edge (16 from the outer border)
const KEY_R: f32 = 5.0;
const CHECK_X: f32 = 4.0;

fn fill() -> Color32 {
    gray(0x24)
}
pub fn line() -> Color32 {
    gray(0xb5)
}
fn label_col(enabled: bool) -> Color32 {
    if enabled { gray(0xb5) } else { gray(0x75) }
}

/// Open a Live-style context menu for `resp` (right-click).
pub fn context(resp: &Response, width: f32, add: impl FnOnce(&mut Ui)) {
    let frame = Frame::new().fill(fill()).stroke(Stroke::new(1.0, line())).inner_margin(0).corner_radius(CornerRadius::ZERO);
    egui::Popup::context_menu(resp).frame(frame).width(width).show(|ui| {
        ui.set_width(width - 2.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        ui.add_space(1.0);
        add(ui);
        ui.add_space(1.0);
    });
}

/// Same frame as `context`, opened by a left click on `resp` and right-aligned
/// below it (the CPU chooser).
pub fn dropdown(resp: &Response, width: f32, add: impl FnOnce(&mut Ui)) {
    let frame = Frame::new().fill(fill()).stroke(Stroke::new(1.0, line())).inner_margin(0).corner_radius(CornerRadius::ZERO);
    egui::Popup::menu(resp).frame(frame).width(width).align(egui::RectAlign::BOTTOM_END).gap(2.0).show(|ui| {
        ui.set_width(width - 2.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        ui.add_space(1.0);
        add(ui);
        ui.add_space(1.0);
    });
}

/// 17 px check row with a 103 × 13 level field (#36 border, #1e inside, orange
/// bar, centred text) 3 px from the right edge. Toggles on click (menu stays open).
pub fn meter_row(ui: &mut Ui, label: &str, checked: bool, level: Option<f32>) -> bool {
    let (rect, resp) = row(ui, 17.0, true);
    let p = ui.painter();
    let y = rect.center().y;
    if checked {
        check_mark(p, pos2(rect.min.x + CHECK_X, y - 3.0), line());
    }
    p.text(pos2(rect.min.x + TEXT_X, y), Align2::LEFT_CENTER, label, font(FS), label_col(true));
    let f = Rect::from_min_size(pos2(rect.max.x - 3.0 - 103.0, rect.min.y + 2.0), vec2(103.0, 13.0));
    p.rect_filled(f, CornerRadius::ZERO, gray(0x36));
    let inner = f.shrink(1.0);
    p.rect_filled(inner, CornerRadius::ZERO, gray(0x1e));
    let text = match level {
        Some(l) => {
            let w = (l.clamp(0.0, 1.0) * inner.width()).floor().max(1.0);
            p.rect_filled(Rect::from_min_size(inner.min, vec2(w, inner.height())), CornerRadius::ZERO, super::theme::ORANGE);
            format!("{:.0} %", l * 100.0)
        }
        None => "---".into(),
    };
    p.text(inner.center(), Align2::CENTER_CENTER, text, font(FS), label_col(true));
    resp.clicked()
}

/// Full-width row; returns its rect and response (hover highlight painted).
fn row(ui: &mut Ui, h: f32, enabled: bool) -> (Rect, Response) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, gray(0x46));
    }
    (rect, resp)
}

fn check_mark(p: &egui::Painter, at: Pos2, col: Color32) {
    // 7 x 6 px tick, as in Live's menus.
    p.line_segment([at + vec2(0.0, 3.0), at + vec2(2.5, 6.0)], Stroke::new(1.2, col));
    p.line_segment([at + vec2(2.5, 6.0), at + vec2(7.0, 0.0)], Stroke::new(1.2, col));
}

/// A menu entry; closes the menu when clicked.
pub fn item(ui: &mut Ui, label: &str, key: &str, enabled: bool) -> bool {
    check_item(ui, label, key, false, enabled)
}

pub fn check_item(ui: &mut Ui, label: &str, key: &str, checked: bool, enabled: bool) -> bool {
    let (rect, resp) = row(ui, ROW, enabled);
    let p = ui.painter();
    let col = label_col(enabled);
    let y = rect.center().y;
    if checked {
        check_mark(p, pos2(rect.min.x + CHECK_X, y - 3.0), col);
    }
    p.text(pos2(rect.min.x + TEXT_X, y), Align2::LEFT_CENTER, label, font(FS), col);
    if !key.is_empty() {
        p.text(pos2(rect.max.x - KEY_R, y), Align2::RIGHT_CENTER, key, font(FS), col);
    }
    let clicked = enabled && resp.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// 1 px line across the menu with Live's spacing around it.
pub fn sep(ui: &mut Ui) {
    ui.add_space(5.0);
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, line());
    ui.add_space(4.0);
}

/// Section caption ("Adaptive Grid:") at the check-mark column.
pub fn caption(ui: &mut Ui, text: &str, h: f32) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    ui.painter().text(pos2(rect.min.x + CHECK_X, rect.min.y + ROW / 2.0), Align2::LEFT_CENTER, text, font(FS), label_col(true));
}

/// One row of radio-like choices at fixed columns (x from the content edge).
/// Returns the index clicked.
pub fn choice_row(ui: &mut Ui, h: f32, items: &[(f32, &str, bool)]) -> Option<usize> {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let mut hit = None;
    for (i, (x, label, checked)) in items.iter().enumerate() {
        let next = items.get(i + 1).map_or(w - 1.0, |n| n.0 - 4.0);
        let cell = Rect::from_min_max(pos2(rect.min.x + x - 12.0, rect.min.y), pos2(rect.min.x + next, rect.min.y + ROW));
        let resp = ui.interact(cell, ui.scope_id().with(("choice", rect.min.y as i32, i)), Sense::click());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(cell, CornerRadius::ZERO, gray(0x46));
        }
        let y = cell.center().y;
        if *checked {
            check_mark(p, pos2(rect.min.x + x - 12.0 + CHECK_X - 3.0, y - 3.0), line());
        }
        p.text(pos2(rect.min.x + x, y), Align2::LEFT_CENTER, *label, font(FS), label_col(true));
        if resp.clicked() {
            hit = Some(i);
        }
    }
    if hit.is_some() {
        ui.close();
    }
    hit
}

/// Live's 70-colour clip/track palette (14 x 5), as in its context menus.
pub const LIVE_COLORS: [[u8; 3]; 70] = [
    [0xff, 0x94, 0xa6],
    [0xff, 0xa5, 0x29],
    [0xcc, 0x99, 0x27],
    [0xf7, 0xf4, 0x7c],
    [0xbf, 0xfb, 0x00],
    [0x1a, 0xff, 0x2f],
    [0x25, 0xff, 0xa8],
    [0x5c, 0xff, 0xe8],
    [0x8b, 0xc5, 0xff],
    [0x54, 0x80, 0xe4],
    [0x92, 0xa7, 0xff],
    [0xd8, 0x6c, 0xe4],
    [0xe5, 0x53, 0xa0],
    [0xff, 0xff, 0xff],
    [0xff, 0x36, 0x36],
    [0xf6, 0x6c, 0x03],
    [0x99, 0x72, 0x4b],
    [0xff, 0xf0, 0x34],
    [0x87, 0xff, 0x67],
    [0x3d, 0xc3, 0x00],
    [0x00, 0xbf, 0xaf],
    [0x19, 0xe9, 0xff],
    [0x10, 0xa4, 0xee],
    [0x00, 0x7d, 0xc0],
    [0x88, 0x6c, 0xe4],
    [0xb6, 0x77, 0xc6],
    [0xff, 0x39, 0xd4],
    [0xd0, 0xd0, 0xd0],
    [0xe2, 0x67, 0x5a],
    [0xff, 0xa3, 0x74],
    [0xd3, 0xad, 0x71],
    [0xed, 0xff, 0xae],
    [0xd2, 0xe4, 0x98],
    [0xba, 0xd0, 0x74],
    [0x9b, 0xc4, 0x8d],
    [0xd4, 0xfd, 0xe1],
    [0xcd, 0xf1, 0xf8],
    [0xb9, 0xc1, 0xe3],
    [0xcd, 0xbb, 0xe4],
    [0xae, 0x98, 0xe5],
    [0xe5, 0xdc, 0xe1],
    [0xa9, 0xa9, 0xa9],
    [0xc6, 0x92, 0x8b],
    [0xb7, 0x82, 0x56],
    [0x99, 0x83, 0x6a],
    [0xbf, 0xba, 0x69],
    [0xa6, 0xbe, 0x00],
    [0x7d, 0xb0, 0x4d],
    [0x88, 0xc2, 0xba],
    [0x9b, 0xb3, 0xc4],
    [0x85, 0xa5, 0xc2],
    [0x83, 0x93, 0xcc],
    [0xa5, 0x95, 0xb5],
    [0xbf, 0x9f, 0xbe],
    [0xbc, 0x71, 0x96],
    [0x7b, 0x7b, 0x7b],
    [0xaf, 0x33, 0x33],
    [0xa9, 0x51, 0x31],
    [0x72, 0x4f, 0x41],
    [0xdb, 0xc3, 0x00],
    [0x85, 0x96, 0x1f],
    [0x53, 0x9f, 0x31],
    [0x0a, 0x9c, 0x8e],
    [0x23, 0x63, 0x84],
    [0x1a, 0x2f, 0x96],
    [0x2f, 0x52, 0xa2],
    [0x62, 0x4b, 0xad],
    [0xa3, 0x4b, 0xad],
    [0xcc, 0x2e, 0x6e],
    [0x3c, 0x3c, 0x3c],
];

/// 14 x 5 swatches, 11 px squares on a 17 px pitch, 9 px from the content edge.
/// Returns the picked colour.
pub fn color_grid(ui: &mut Ui, current: [u8; 3]) -> Option<[u8; 3]> {
    ui.add_space(2.0);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 5.0 * 17.0 - 6.0), Sense::hover());
    let mut hit = None;
    for (i, c) in LIVE_COLORS.iter().enumerate() {
        let (col, rw) = ((i % 14) as f32, (i / 14) as f32);
        let r = Rect::from_min_size(pos2(rect.min.x + 9.0 + col * 17.0, rect.min.y + rw * 17.0), vec2(11.0, 11.0));
        let resp = ui.interact(r.expand(3.0), ui.scope_id().with(("swatch", i)), Sense::click());
        let p = ui.painter();
        p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(c[0], c[1], c[2]));
        if *c == current || resp.hovered() {
            p.rect_stroke(r.expand(2.0), CornerRadius::ZERO, Stroke::new(1.0, line()), egui::StrokeKind::Inside);
        }
        if resp.clicked() {
            hit = Some(*c);
        }
    }
    ui.add_space(2.0);
    if hit.is_some() {
        ui.close();
    }
    hit
}
