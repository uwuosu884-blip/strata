//! Crisp lines at any interface scale.
//!
//! egui already re-rasterises text and rounds rects / straight line segments to
//! the pixel grid, but at a fractional scale (e.g. 137 %) a 1-point stroke is
//! 1.37 px wide and gets soft, feathered edges. This plugin runs on every frame's
//! output, before tessellation, and rounds stroke widths to whole pixels;
//! axis-aligned paths (outlines drawn as polylines) also get their corners put
//! on pixel centres. At whole-number scales nothing is touched.

use eframe::egui::{self, Pos2, Shape, epaint::StrokeKind};

pub struct PixelSnap;

impl egui::Plugin for PixelSnap {
    fn debug_name(&self) -> &'static str {
        "daw_pixel_snap"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, out: &mut egui::FullOutput) {
        let ppp = out.pixels_per_point;
        if !needs_snap(ppp) {
            return;
        }
        for cs in &mut out.shapes {
            snap_shape(&mut cs.shape, ppp);
        }
    }
}

fn needs_snap(ppp: f32) -> bool {
    ppp > 0.0 && (ppp - ppp.round()).abs() > 1e-4
}

/// Stroke width in points → a whole number of pixels (at least 1 px for
/// strokes of half a pixel or more; thinner hairlines stay as they are).
pub fn snap_width(w: f32, ppp: f32) -> f32 {
    let px = w * ppp;
    if px < 0.5 { w } else { px.round().max(1.0) / ppp }
}

/// Centre of a stroke `w_px` pixels wide: pixel centre for odd widths, pixel
/// edge for even ones (so the stroke covers whole pixels).
fn snap_center(v: f32, w_px: f32, ppp: f32) -> f32 {
    let p = v * ppp;
    if (w_px.round() as i32) % 2 == 1 { ((p - 0.5).round() + 0.5) / ppp } else { p.round() / ppp }
}

fn snap_shape(s: &mut Shape, ppp: f32) {
    match s {
        Shape::Vec(v) => v.iter_mut().for_each(|s| snap_shape(s, ppp)),
        Shape::LineSegment { stroke, .. } => stroke.width = snap_width(stroke.width, ppp),
        Shape::Rect(r) => {
            if r.stroke.width > 0.0 {
                r.stroke.width = snap_width(r.stroke.width, ppp);
            }
        }
        Shape::Circle(c) => {
            if c.stroke.width > 0.0 {
                c.stroke.width = snap_width(c.stroke.width, ppp);
            }
        }
        Shape::Ellipse(e) => {
            if e.stroke.width > 0.0 {
                e.stroke.width = snap_width(e.stroke.width, ppp);
            }
        }
        Shape::Path(p) => {
            if p.stroke.width <= 0.0 {
                return;
            }
            p.stroke.width = snap_width(p.stroke.width, ppp);
            // Only axis-aligned outlines; slanted / curved paths stay smooth.
            if p.points.len() >= 2 && p.fill == egui::Color32::TRANSPARENT && p.stroke.kind == StrokeKind::Middle {
                let straight = p.points.windows(2).all(|w| w[0].x == w[1].x || w[0].y == w[1].y);
                if straight {
                    let w_px = p.stroke.width * ppp;
                    for q in &mut p.points {
                        *q = Pos2::new(snap_center(q.x, w_px, ppp), snap_center(q.y, w_px, ppp));
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, Stroke, pos2};

    #[test]
    fn widths_become_whole_pixels() {
        for ppp in [1.13f32, 1.37, 1.5, 1.75, 0.83] {
            for w in [1.0f32, 1.3, 2.0] {
                let px = snap_width(w, ppp) * ppp;
                assert!((px - px.round()).abs() < 1e-4, "{w} pt at {ppp}: {px} px");
                assert!(px >= 1.0);
            }
        }
        // Sub-half-pixel hairlines are left alone.
        assert_eq!(snap_width(0.3, 1.37), 0.3);
    }

    #[test]
    fn integer_scales_untouched() {
        assert!(!needs_snap(1.0));
        assert!(!needs_snap(2.0));
        assert!(needs_snap(1.25));
    }

    #[test]
    fn axis_aligned_path_lands_on_pixel_centres() {
        let ppp = 1.37;
        let mut s = Shape::line(vec![pos2(10.0, 10.0), pos2(30.0, 10.0), pos2(30.0, 20.0)], Stroke::new(1.0, Color32::WHITE));
        snap_shape(&mut s, ppp);
        let Shape::Path(p) = s else { panic!() };
        for q in &p.points {
            for v in [q.x, q.y] {
                let f = (v * ppp).fract();
                assert!((f - 0.5).abs() < 1e-3, "{v} → {}", v * ppp);
            }
        }
    }
}
