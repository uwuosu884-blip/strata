//! Live's mouse cursors (Cursors.tar, embedded), drawn in software: egui has no
//! custom cursor images, so the OS cursor is hidden while one of these shows.
//! Widgets call `set` each frame; egui's own resize/grab icons map automatically.

use eframe::egui::{self, Color32, ColorImage, CursorIcon, Id, LayerId, Order, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // the full Live set; not every tool uses each one yet
pub enum Cur {
    AddBreakpoint,
    Chop,
    ClosedHand,
    Convert,
    Curve,
    DragCopy,
    Drag,
    DragLeftEdge,
    DragRightEdge,
    Pencil,
    InMarker,
    OpenHand,
    OutMarker,
    PointingHand,
    ResizeLeftRight,
    Slice,
    SliceFromEnd,
    Speaker,
    StretchLeftEdge,
    StretchRightEdge,
    Zoom,
}

const FILES: [&[u8]; 21] = [
    include_bytes!("../../assets/cursors/AddBreakpointCursor.cur"),
    include_bytes!("../../assets/cursors/ChopCursor.cur"),
    include_bytes!("../../assets/cursors/ClosedHandCursor.cur"),
    include_bytes!("../../assets/cursors/ConvertCursor.cur"),
    include_bytes!("../../assets/cursors/CurveCursor.cur"),
    include_bytes!("../../assets/cursors/DragCopyCursor.cur"),
    include_bytes!("../../assets/cursors/DragCursor.cur"),
    include_bytes!("../../assets/cursors/DragLeftEdgeCursor.cur"),
    include_bytes!("../../assets/cursors/DragRightEdgeCursor.cur"),
    include_bytes!("../../assets/cursors/DrawingPencilCursor.cur"),
    include_bytes!("../../assets/cursors/InMarkerCursor.cur"),
    include_bytes!("../../assets/cursors/OpenHandCursor.cur"),
    include_bytes!("../../assets/cursors/OutMarkerCursor.cur"),
    include_bytes!("../../assets/cursors/PointingHandCursor.cur"),
    include_bytes!("../../assets/cursors/ResizeLeftRightWithBar.cur"),
    include_bytes!("../../assets/cursors/SliceCursor.cur"),
    include_bytes!("../../assets/cursors/SliceFromEndCursor.cur"),
    include_bytes!("../../assets/cursors/SpeakerCursor.cur"),
    include_bytes!("../../assets/cursors/StretchLeftEdgeCursor.cur"),
    include_bytes!("../../assets/cursors/StretchRightEdgeCursor.cur"),
    include_bytes!("../../assets/cursors/ZoomCursor.cur"),
];

/// A decoded cursor image: RGBA pixels (top row first) and its hotspot.
pub struct CurImage {
    pub size: [usize; 2],
    pub rgba: Vec<Color32>,
    pub hotspot: [u16; 2],
}

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// Decode the `want`-px image of a Windows `.cur` (BMP payload, 24/32 bpp,
/// rows bottom-up; 24 bpp uses the AND mask for transparency).
pub fn decode(file: &[u8], want: u32) -> Option<CurImage> {
    if u16le(file, 2)? != 2 {
        return None;
    }
    let count = u16le(file, 4)? as usize;
    let mut best: Option<(u32, usize)> = None;
    for i in 0..count {
        let e = 6 + i * 16;
        let w = match *file.get(e)? {
            0 => 256,
            w => w as u32,
        };
        if best.is_none_or(|(bw, _)| bw.abs_diff(want) > w.abs_diff(want)) {
            best = Some((w, e));
        }
    }
    let (_, e) = best?;
    let hotspot = [u16le(file, e + 4)?, u16le(file, e + 6)?];
    let off = u32le(file, e + 12)? as usize;
    let bmp = file.get(off..)?;
    let hdr = u32le(bmp, 0)? as usize;
    let w = u32le(bmp, 4)? as usize;
    let h = (u32le(bmp, 8)? as usize) / 2; // XOR + AND halves
    let bpp = u16le(bmp, 14)? as usize;
    if bpp != 24 && bpp != 32 {
        return None;
    }
    let stride = (w * bpp / 8).div_ceil(4) * 4;
    let mask_stride = w.div_ceil(32) * 4;
    let px = bmp.get(hdr..)?;
    let mask = px.get(stride * h..)?;
    let mut rgba = vec![Color32::TRANSPARENT; w * h];
    for y in 0..h {
        let src = (h - 1 - y) * stride;
        for x in 0..w {
            let o = src + x * bpp / 8;
            let (b, g, r) = (*px.get(o)?, *px.get(o + 1)?, *px.get(o + 2)?);
            let a = if bpp == 32 {
                *px.get(o + 3)?
            } else {
                let m = mask.get((h - 1 - y) * mask_stride + x / 8)?;
                if m & (0x80 >> (x % 8)) != 0 { 0 } else { 255 }
            };
            rgba[y * w + x] = Color32::from_rgba_unmultiplied(r, g, b, a);
        }
    }
    Some(CurImage { size: [w, h], rgba, hotspot })
}

fn chosen_id() -> Id {
    Id::unique("daw_custom_cursor")
}
fn tex_id() -> Id {
    Id::unique("daw_cursor_textures")
}

/// Show `c` this frame (call while hovering / dragging).
pub fn set(ctx: &egui::Context, c: Cur) {
    ctx.data_mut(|d| d.insert_temp(chosen_id(), c));
}

/// egui's own icons that have a Live counterpart.
fn map_icon(i: CursorIcon) -> Option<Cur> {
    Some(match i {
        CursorIcon::ResizeHorizontal | CursorIcon::ResizeColumn | CursorIcon::ResizeEast | CursorIcon::ResizeWest => Cur::ResizeLeftRight,
        CursorIcon::Grab => Cur::OpenHand,
        CursorIcon::Grabbing => Cur::ClosedHand,
        CursorIcon::PointingHand => Cur::PointingHand,
        CursorIcon::ZoomIn | CursorIcon::ZoomOut => Cur::Zoom,
        CursorIcon::Copy => Cur::DragCopy,
        CursorIcon::Move => Cur::Drag,
        _ => return None,
    })
}

/// Paint the chosen cursor (call once, last in the frame).
pub fn paint(ctx: &egui::Context) {
    let chosen = ctx.data_mut(|d| {
        let c = d.get_temp::<Cur>(chosen_id());
        d.remove::<Cur>(chosen_id());
        c
    });
    let Some(c) = chosen.or_else(|| map_icon(ctx.output(|o| o.cursor_icon))) else { return };
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
    if !ctx.content_rect().contains(pos) {
        return;
    }
    let texs: Vec<Option<(TextureHandle, [u16; 2])>> = ctx.data_mut(|d| d.get_temp(tex_id())).unwrap_or_else(|| {
        let t: Vec<_> = FILES
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let im = decode(f, 32)?;
                let img = ColorImage::new(im.size, im.rgba);
                Some((ctx.load_texture(format!("cursor{i}"), img, TextureOptions::NEAREST), im.hotspot))
            })
            .collect();
        ctx.data_mut(|d| d.insert_temp(tex_id(), t.clone()));
        t
    });
    let Some(Some((tex, hs))) = texs.get(c as usize) else { return };
    ctx.set_cursor_icon(CursorIcon::None);
    let size = tex.size_vec2() / ctx.pixels_per_point();
    let ppp = ctx.pixels_per_point();
    let min = pos - vec2(hs[0] as f32, hs[1] as f32) / ppp;
    let min = pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
    let p = ctx.layer_painter(LayerId::new(Order::Debug, Id::unique("daw_cursor_layer")));
    p.image(tex.id(), Rect::from_min_size(min, size), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_cursors_decode_with_hotspots() {
        for (i, f) in FILES.iter().enumerate() {
            let im = decode(f, 32).unwrap_or_else(|| panic!("cursor {i}"));
            assert_eq!(im.size, [32, 32]);
            assert!(im.rgba.iter().any(|c| c.a() == 255), "cursor {i} has opaque pixels");
            assert!(im.rgba.iter().any(|c| c.a() == 0), "cursor {i} has transparency");
        }
        assert_eq!(decode(FILES[Cur::PointingHand as usize], 32).unwrap().hotspot, [4, 0]);
        assert_eq!(decode(FILES[Cur::Pencil as usize], 32).unwrap().hotspot, [1, 14]);
        assert_eq!(decode(FILES[Cur::OutMarker as usize], 32).unwrap().hotspot, [12, 7]);
        assert_eq!(decode(FILES[Cur::Slice as usize], 32).unwrap().size, [32, 32]);
    }
}
