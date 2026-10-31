//! Text colour pass (0.7.12): pure black text is lifted to rgb(32,32,32) and
//! near-pure white text (brighter than the target in every channel) becomes
//! the warm off-white rgb(244,240,235). Runs on every frame's output before
//! tessellation, so every text — painter, widgets, menus — gets it; other
//! shapes and lighter greys are untouched.

use eframe::egui::{self, Color32, Shape, epaint::TextShape};
use std::sync::Arc;

pub const BLACK: Color32 = Color32::from_rgb(32, 32, 32);
pub const WHITE: Color32 = Color32::from_rgb(244, 240, 235);

pub struct TextColor;

impl egui::Plugin for TextColor {
    fn debug_name(&self) -> &'static str {
        "daw_text_color"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, out: &mut egui::FullOutput) {
        for cs in &mut out.shapes {
            fix_shape(&mut cs.shape);
        }
    }
}

/// Colour mapping (alpha kept).
pub fn map(c: Color32) -> Color32 {
    if c == Color32::PLACEHOLDER || c.a() == 0 {
        return c;
    }
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let to = if r < BLACK.r() && g < BLACK.g() && b < BLACK.b() {
        BLACK
    } else if r >= WHITE.r() && g >= WHITE.g() && b >= WHITE.b() {
        WHITE
    } else {
        return c;
    };
    Color32::from_rgba_unmultiplied(to.r(), to.g(), to.b(), a)
}

fn fix_shape(s: &mut Shape) {
    match s {
        Shape::Vec(v) => v.iter_mut().for_each(fix_shape),
        Shape::Text(t) => fix_text(t),
        _ => {}
    }
}

fn fix_text(t: &mut TextShape) {
    if let Some(c) = t.override_text_color {
        t.override_text_color = Some(map(c));
        return;
    }
    t.fallback_color = map(t.fallback_color);
    // Section colours are baked into the glyph vertices.
    fn needs(r: &egui::epaint::text::Row) -> bool {
        r.visuals.mesh.vertices[r.visuals.glyph_vertex_range.clone()].iter().any(|v| map(v.color) != v.color)
    }
    if !t.galley.rows.iter().any(|r| needs(&r.row)) {
        return;
    }
    let g = Arc::make_mut(&mut t.galley);
    for pr in &mut g.rows {
        let row = Arc::make_mut(&mut pr.row);
        let range = row.visuals.glyph_vertex_range.clone();
        for v in &mut row.visuals.mesh.vertices[range] {
            v.color = map(v.color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_extremes() {
        assert_eq!(map(Color32::BLACK), BLACK);
        assert_eq!(map(Color32::from_gray(0x10)), BLACK);
        assert_eq!(map(Color32::WHITE), WHITE);
        assert_eq!(map(Color32::from_gray(0xf6)), WHITE);
        // Light greys and colours stay.
        for c in [Color32::from_gray(0xf0), Color32::from_gray(0xb5), Color32::from_gray(0x20), Color32::from_rgb(255, 160, 40)] {
            assert_eq!(map(c), c);
        }
        assert_eq!(map(Color32::PLACEHOLDER), Color32::PLACEHOLDER);
        assert_eq!(map(Color32::from_rgba_unmultiplied(255, 255, 255, 128)).a(), 128);
    }
}
