//! Detail panel: clip editor (piano roll / audio clip) or the selected track's device.

use super::theme::*;
use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem};
use daw_engine::{DRUM_BASE_NOTE, DRUM_PADS, Wave};
use daw_project::{Clip, ClipContent, InstrumentDef, Note, PadDef, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

/// Key column: note names 0..29, keys 30..56 (Live 12).
const KEYS_W: f32 = 57.0;
const NAMES_W: f32 = 30.0;
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn note_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p as i32 / 12 - 2)
}

pub fn detail_panel(app: &mut DawApp, ui: &mut egui::Ui) {
    // A resizable panel stores its *content* size: fill exactly the user-set size so
    // switching between the (tall) piano roll and a short device view never changes it.
    let full = ui.max_rect().size();
    ui.set_min_size(full);
    ui.set_max_size(full);
    if app.master_selected || app.selected_return.is_some() {
        device_view(app, ui);
        return;
    }
    if app.project.tracks.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("Add a track: Ctrl+T (audio) or Ctrl+Shift+T (MIDI)").color(Color32::GRAY))
        });
        return;
    }
    // Clip view only for a clip on the selected track (selecting another track,
    // e.g. by its header, shows that track's devices). Shift+Tab toggles.
    let clip = app.selected_clip.filter(|&c| {
        let t = match c {
            ClipRef::Slot { track, .. } | ClipRef::Arr { track, .. } => track,
        };
        t == app.selected_track && app.clip(c).is_some()
    });
    match (app.detail_mode, clip) {
        (crate::app::DetailMode::Clip, Some(c)) => clip_editor(app, ui, c),
        (crate::app::DetailMode::Clip, None) => {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No clip selected on this track · Shift+Tab: devices").color(Color32::GRAY))
            });
        }
        (crate::app::DetailMode::Device, _) => device_view(app, ui),
    }
}

// ============================ Clip editor ============================

fn clip_editor(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef) {
    let track = match cref {
        ClipRef::Slot { track, .. } | ClipRef::Arr { track, .. } => track,
    };
    let clip = app.clip(cref).cloned().expect("checked");
    // Live 12 layout: 170 px left panel | 2 px #24 | editor (17 px header + 1 px sep).
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    let mut ed_r = rect;
    if !app.cv.folded {
        let left = Rect::from_min_size(rect.min, vec2(super::cview::LEFT_W, rect.height()));
        super::cview::left_panel(app, ui, left, cref, &clip);
        ed_r.min.x = left.max.x + 2.0;
    }
    ui.painter().rect_filled(
        Rect::from_min_max(pos2(ed_r.min.x - 2.0, rect.min.y), pos2(ed_r.min.x, rect.max.y)),
        CornerRadius::ZERO,
        super::theme::sep(),
    );
    let head = Rect::from_min_size(ed_r.min, vec2(ed_r.width(), super::cview::HEAD_H));
    let (info, grid) = match &clip.content {
        ClipContent::Midi { .. } => (None, Some(app.pr_grid_now)),
        ClipContent::Audio { sample, warp_bpm, .. } => (Some(sample_info(app, ui, sample)), warp_bpm.map(|_| app.pr_grid_now)),
    };
    super::cview::editor_header(app, ui, head, &clip, info, grid);
    let body = Rect::from_min_max(pos2(ed_r.min.x, head.max.y + 1.0), ed_r.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(body.intersect(ui.clip_rect()));
    let ui = &mut child;
    match &clip.content {
        ClipContent::Midi { notes } => piano_roll(app, ui, cref, track, &clip, notes),
        ClipContent::Audio { .. } => super::clip::sample_editor(app, ui, cref, &clip),
    }
}

/// "name.wav  48.0 kHz · 32-Bit · 2 Ch" (format probed once per file).
fn sample_info(app: &DawApp, ui: &egui::Ui, path: &str) -> String {
    #[allow(deprecated)]
    let id = egui::Id::new(("sample_info", path));
    if let Some(s) = ui.data(|d| d.get_temp::<String>(id)) {
        return s;
    }
    let name = std::path::Path::new(path).file_name().map_or(path.to_string(), |n| n.to_string_lossy().into_owned());
    let Some(buf) = app.cache.peek(path) else { return name };
    let khz = buf.sample_rate as f64 / 1000.0;
    let s = match hound::WavReader::open(path).map(|r| r.spec()) {
        Ok(sp) => format!("{name}  {khz:.1} kHz · {}-Bit · {} Ch", sp.bits_per_sample, sp.channels),
        Err(_) => format!("{name}  {khz:.1} kHz · {} Ch", if buf.left == buf.right { 1 } else { 2 }),
    };
    ui.data_mut(|d| d.insert_temp(id, s.clone()));
    s
}

#[derive(Clone, Copy, Debug)]
enum NoteDrag {
    Move {
        grab_beat: f64,
        grab_pitch: i32,
        orig: Option<(f64, u8)>,
    },
    /// Note-end drag; `grab` = the note under the pointer. Every selected note
    /// keeps its own length and changes by the same delta.
    Resize {
        grab: usize,
    },
    Velocity,
    /// Rubber-band selection; `base` = selection kept when Shift/Ctrl was held.
    Marquee,
    /// Playhead scrub in the ruler.
    Ruler,
}

const PR_RULER_H: f32 = 16.0;
/// Velocity bar under the piano roll (Live 12).
const VBAR_H: f32 = 18.0;
/// How far the piano roll view may grow beyond the clip either side (beats).
const MAX_VIEW_BEATS: f64 = 4096.0;

/// Beats the piano roll must show: 0, the markers, the loop and every note.
fn content_span(clip: &Clip, notes: &[Note]) -> (f64, f64) {
    let (w0, w1) = super::clip::play_window(clip);
    let (l0, l1) = clip.loop_range();
    notes.iter().fold((w0.min(l0).min(0.0), w1.max(l1).max(clip.length.max(0.25))), |(a, b), n| (a.min(n.start), b.max(n.start + n.length)))
}

/// Ruler bar number of the bar starting at beat `b`: 1, 2, … and -1, -2, …
/// before the clip start (Live has no bar 0).
fn bar_label(b: f64, bpb: f64) -> String {
    let k = (b / bpb + 1e-9).floor() as i64;
    if k >= 0 { format!("{}", k + 1) } else { format!("{k}") }
}

fn piano_roll(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, track: usize, clip: &Clip, notes: &[Note]) {
    // Notes tab: a 17 px velocity bar at the bottom (Live 12) — the lane's
    // collapse button, Randomize, Ramp and Deviation.
    let has_bar = app.cv.tab == super::clip::Tab::Main;
    let avail = ui.available_size();
    let (rect, resp) =
        ui.allocate_exact_size(vec2(avail.x, (avail.y - if has_bar { VBAR_H } else { 0.0 }).max(40.0)), Sense::click_and_drag());
    let bar = Rect::from_min_size(pos2(rect.min.x, rect.max.y), vec2(rect.width(), VBAR_H));
    let p = ui.painter_at(rect);
    // Live colours (no #1a): panel grey around, the velocity lane like the grid.
    p.rect_filled(rect, CornerRadius::ZERO, panel());
    let drums: Option<Vec<String>> = match &app.project.tracks[track].instrument {
        InstrumentDef::Drums { pads } => Some(pads.iter().map(|d| d.name.clone()).collect()),
        _ => None,
    };
    // Velocity lane height: user-resizable via the splitter above it.
    let lane_on = !(has_bar && app.pr_vel_collapsed);
    let vel_h = if lane_on { app.pr_vel_h.clamp(24.0, (rect.height() * 0.6).max(24.0)) } else { 0.0 };
    // Live 12 rows: bar numbers 0..16, loop / marker rows 17..38, sep, scrub strip
    // 40..55, sep, note grid from 57.
    let nums = Rect::from_min_max(pos2(rect.min.x + KEYS_W, rect.min.y), pos2(rect.max.x, rect.min.y + 17.0));
    let marker = Rect::from_min_max(pos2(rect.min.x + KEYS_W, nums.max.y), pos2(rect.max.x, nums.max.y + 22.0));
    let ruler = Rect::from_min_max(pos2(rect.min.x + KEYS_W, marker.max.y + 1.0), pos2(rect.max.x, marker.max.y + 1.0 + PR_RULER_H));
    let grid_area = Rect::from_min_max(pos2(rect.min.x + KEYS_W, ruler.max.y + 1.0), pos2(rect.max.x, rect.max.y - vel_h));
    let vel_area = Rect::from_min_max(pos2(rect.min.x + KEYS_W, rect.max.y - vel_h + 3.0), pos2(rect.max.x, rect.max.y - 2.0));
    p.rect_filled(Rect::from_min_max(pos2(grid_area.min.x, grid_area.max.y), rect.max), CornerRadius::ZERO, gray(0x39));
    let len = clip.length.max(0.25);
    // Markers: notes outside [w0, w1) are kept but silent (shaded area). The
    // view spans the markers and every note, either side.
    let (w0, w1) = super::clip::play_window(clip);
    let bpb = app.project.beats_per_bar as f64;

    // On selecting a different clip: reset zoom/scroll and show its notes (or the drum pads).
    let seen_id = ui.scope_id().with("pr_clip");
    let fresh = ui.data(|d| d.get_temp::<u64>(seen_id)) != Some(clip.id);
    if fresh {
        ui.data_mut(|d| d.insert_temp(seen_id, clip.id));
        app.note_sel.clear();
        app.pr_time_sel = None;
        app.pr_zoom = 0.0;
        app.pr_scroll = 0.0;
        app.pr_hold = None;
    }
    // Zoom-out limit: content extent, widened by `pr_hold` (where notes were
    // before being moved inward) until the user zooms in.
    let (content_lo, content_hi) = content_span(clip, notes);
    let (view_lo, view_hi) = match app.pr_hold {
        Some((a, b)) => (content_lo.min(a), content_hi.max(b)),
        None => (content_lo, content_hi),
    };
    let view_len = (view_hi - view_lo).max(0.25);
    let mut row_h = app.pr_row_h.clamp(5.0, 32.0);
    let mut rows = (grid_area.height() / row_h).floor().max(1.0) as i32;
    if fresh {
        app.pr_low_note = if drums.is_some() {
            DRUM_BASE_NOTE as i32
        } else if notes.is_empty() {
            48
        } else {
            let (lo, hi) = notes.iter().fold((127i32, 0i32), |(l, h), n| (l.min(n.pitch as i32), h.max(n.pitch as i32)));
            ((lo + hi) / 2 - rows / 2).clamp(0, (127 - rows).max(0))
        };
    }

    // Wheel: plain = pitch scroll, Shift = horizontal scroll, Ctrl = horizontal zoom
    // (around the pointer), Alt = row height.
    let fit = (grid_area.width() as f64 / view_len).max(1e-3);
    let mut ppb = if app.pr_zoom > 0.0 { (app.pr_zoom as f64).max(fit) } else { fit };
    if resp.hovered() {
        let (zoom, scroll, alt, hover) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.alt, i.pointer.hover_pos()));
        // Zoom gestures anchor at the point under the cursor when the gesture
        // STARTED (a gesture ends after 0.35 s without zoom input): that beat /
        // pitch stays at its screen position however the cursor moves meanwhile.
        let now = ui.input(|i| i.time);
        let live = |a: &Option<(f64, f32, f64)>| a.filter(|a| now - a.2 < 0.35);
        if zoom != 1.0
            && let Some(h) = hover
        {
            let (beat, off) = match live(&app.pr_zoom_anchor) {
                Some((b, o, _)) => (b, o),
                None => {
                    let o = (h.x - grid_area.min.x).max(0.0);
                    (app.pr_scroll + o as f64 / ppb, o)
                }
            };
            app.pr_zoom_anchor = Some((beat, off, now));
            if zoom > 1.0 {
                app.pr_hold = None; // zooming in forgets where moved notes were
            }
            ppb = (ppb * zoom as f64).clamp(fit, 600.0f64.max(fit));
            app.pr_zoom = if ppb <= fit * 1.0001 { 0.0 } else { ppb as f32 };
            app.pr_scroll = beat - off as f64 / ppb;
        } else if alt && scroll.y != 0.0 {
            let new_h = (row_h * (1.0 + scroll.y * 0.004)).clamp(5.0, 32.0);
            app.pr_row_h = new_h;
            if let Some(h) = hover {
                let (fpitch, below) = match live(&app.pr_row_anchor) {
                    Some((p, b, _)) => (p as f32, b),
                    None => {
                        let below = (grid_area.max.y - h.y).max(0.0);
                        (app.pr_low_note as f32 + below / row_h, below)
                    }
                };
                app.pr_row_anchor = Some((fpitch as f64, below, now));
                let new_rows = (grid_area.height() / new_h).floor().max(1.0) as i32;
                app.pr_low_note = (fpitch - below / new_h).round().clamp(0.0, (127 - new_rows + 1).max(0) as f32) as i32;
            }
        } else {
            if scroll.x != 0.0 {
                app.pr_scroll -= scroll.x as f64 / ppb;
            }
            if scroll.y != 0.0 {
                app.pr_low_note = (app.pr_low_note + (scroll.y / row_h).round() as i32).clamp(0, 127 - rows.min(127));
            }
        }
    }
    row_h = app.pr_row_h.clamp(5.0, 32.0);
    rows = (grid_area.height() / row_h).floor().max(1.0) as i32;
    let visible = grid_area.width() as f64 / ppb;
    app.pr_scroll = if app.pr_zoom > 0.0 { app.pr_scroll.clamp(view_lo, (view_hi - visible).max(view_lo)) } else { view_lo };
    let scroll = app.pr_scroll;
    let low = app.pr_low_note.clamp(0, (127 - rows + 1).max(0));
    app.pr_low_note = low;

    let grid = app.pr_grid.step(ppb, bpb).min(len.max(w1 - w0));
    app.pr_grid_now = grid;
    let free = ui.input(|i| i.modifiers.alt) || !app.pr_grid.snaps();
    let x_of = |b: f64| grid_area.min.x + ((b - scroll) * ppb) as f32;
    let y_of = |pitch: i32| grid_area.max.y - (pitch - low + 1) as f32 * row_h;
    let beat_at = |x: f32| scroll + ((x - grid_area.min.x) as f64) / ppb;
    let pitch_at = |y: f32| low + ((grid_area.max.y - y) / row_h).floor() as i32;
    // Alt (or grid Off) = place freely.
    let snap = |b: f64| if free { b } else { (b / grid).floor() * grid };
    let snap_round = |b: f64| if free { b } else { (b / grid).round() * grid };

    // Rows + keys
    for r in 0..rows {
        let pitch = low + r;
        if pitch > 127 {
            break;
        }
        let y = y_of(pitch);
        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        // Scale mode: in-scale rows lit, root rows tinted (Live 12).
        let row_c = if clip.scale.on {
            if (pitch - clip.scale.root as i32).rem_euclid(12) == 0 {
                Color32::from_rgb(52, 50, 40)
            } else if clip.scale.contains(pitch) {
                gray(0x39)
            } else {
                gray(0x2c)
            }
        } else if black {
            gray(0x32)
        } else {
            gray(0x39)
        };
        p.rect_filled(Rect::from_min_max(pos2(grid_area.min.x, y), pos2(grid_area.max.x, y + row_h)), CornerRadius::ZERO, row_c);
        // Live: 1 px separators between B/C and E/F.
        if matches!(pitch % 12, 0 | 5) {
            p.line_segment([pos2(grid_area.min.x, y + row_h - 0.5), pos2(grid_area.max.x, y + row_h - 0.5)], Stroke::new(1.0, gray(0x34)));
        }
        let key_x0 = if drums.is_some() { rect.min.x } else { rect.min.x + NAMES_W };
        let key = Rect::from_min_max(pos2(key_x0, y), pos2(rect.min.x + KEYS_W - 1.0, y + row_h - 1.0));
        p.rect_filled(Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.min.x + NAMES_W, y + row_h)), CornerRadius::ZERO, panel());
        p.rect_filled(key, CornerRadius::ZERO, if black { gray(0x46) } else { gray(0x86) });
        let label = match &drums {
            Some(names) if pitch >= DRUM_BASE_NOTE as i32 && ((pitch - DRUM_BASE_NOTE as i32) as usize) < names.len() => {
                Some(names[(pitch - DRUM_BASE_NOTE as i32) as usize].clone())
            }
            _ if pitch % 12 == 0 => Some(note_name(pitch as u8)),
            _ => None,
        };
        if let Some(l) = label
            && row_h >= 7.0
        {
            if drums.is_some() {
                p.with_clip_rect(key).text(
                    key.left_center() + vec2(2.0, 0.0),
                    Align2::LEFT_CENTER,
                    l,
                    super::theme::font(FS.min(row_h - 1.0)),
                    gray(0x12),
                );
            } else {
                // Note names sit left of the keys, bottom-aligned on the C row.
                p.text(pos2(rect.min.x + 3.0, y + row_h - 1.0), Align2::LEFT_BOTTOM, l, super::theme::font(FS), text());
            }
        }
    }
    // Ruler + beat lines (thinned when the grid gets denser than ~5 px).
    let gp = ui.painter_at(Rect::from_min_max(nums.min, rect.max));
    let np0 = ui.painter_at(nums);
    np0.rect_filled(nums, CornerRadius::ZERO, gray(0x39));
    gp.rect_filled(Rect::from_min_max(pos2(rect.min.x, marker.max.y), pos2(rect.max.x, marker.max.y + 1.0)), CornerRadius::ZERO, sep());
    gp.rect_filled(ruler, CornerRadius::ZERO, gray(0x30));
    gp.rect_filled(Rect::from_min_max(pos2(rect.min.x, ruler.max.y), pos2(rect.max.x, ruler.max.y + 1.0)), CornerRadius::ZERO, sep());
    // Corner above the keys.
    p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.min.x + KEYS_W, ruler.max.y)), CornerRadius::ZERO, panel());
    p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.min.x + KEYS_W, nums.max.y)), CornerRadius::ZERO, gray(0x46));
    // Live: every other bar is a lighter band.
    {
        let mut bar = (scroll / bpb).floor();
        while bar * bpb <= (scroll + visible).min(view_hi) {
            if (bar as i64).rem_euclid(2) == 1 {
                let r = Rect::from_min_max(pos2(x_of(bar * bpb), grid_area.min.y), pos2(x_of((bar + 1.0) * bpb), grid_area.max.y));
                gp.rect_filled(r, CornerRadius::ZERO, Color32::from_white_alpha(5));
                np0.rect_filled(Rect::from_min_max(pos2(r.min.x, nums.min.y), pos2(r.max.x, nums.max.y)), CornerRadius::ZERO, gray(0x3e));
            }
            bar += 1.0;
        }
    }
    let line_step = if grid * ppb < 5.0 { crate::arr_ops::adaptive_step(ppb, bpb, 8.0) } else { grid };
    let mut b = (scroll / line_step).floor() * line_step;
    let end = (scroll + visible).min(view_hi);
    while b <= end + 1e-9 {
        let bar = (b / bpb).fract().abs() < 1e-9;
        let c = if bar {
            gray(0x24)
        } else if b.fract().abs() < 1e-9 {
            gray(0x2c)
        } else {
            gray(0x33)
        };
        let x = x_of(b);
        gp.line_segment([pos2(x, grid_area.min.y), pos2(x, rect.max.y)], Stroke::new(1.0, c));
        if bar {
            // "[" tick: 9 px stem + 2 px foot at its top.
            np0.line_segment([pos2(x + 0.5, nums.max.y - 9.0), pos2(x + 0.5, nums.max.y)], Stroke::new(1.0, gray(0x91)));
            np0.line_segment([pos2(x + 0.5, nums.max.y - 8.5), pos2(x + 2.5, nums.max.y - 8.5)], Stroke::new(1.0, gray(0x91)));
            np0.text(pos2(x + 4.0, nums.max.y - 5.5), Align2::LEFT_CENTER, bar_label(b, bpb), super::theme::font(FS), text());
        } else if b.fract().abs() < 1e-9 && line_step * ppb >= 40.0 {
            np0.line_segment([pos2(x + 0.5, nums.max.y - 9.0), pos2(x + 0.5, nums.max.y)], Stroke::new(1.0, gray(0x91)));
            np0.line_segment([pos2(x + 0.5, nums.max.y - 8.5), pos2(x + 2.5, nums.max.y - 8.5)], Stroke::new(1.0, gray(0x91)));
            let beat = (b - (b / bpb).floor() * bpb).round() as i64 + 1;
            np0.text(
                pos2(x + 4.0, nums.max.y - 5.5),
                Align2::LEFT_CENTER,
                format!("{}.{}", bar_label((b / bpb).floor() * bpb, bpb), beat),
                super::theme::font(FS),
                text(),
            );
        }
        b += line_step;
    }
    // Outside the markers (notes there are kept but silent): Live shades the
    // rows and bar numbers just 3 levels darker (hn-ni-ni) and draws #91 lines
    // at the start / end markers.
    let shade_out = Color32::from_black_alpha(14);
    for (a, z) in [(grid_area.min.x, x_of(w0)), (x_of(w1), grid_area.max.x)] {
        let (a, z) = (a.max(grid_area.min.x), z.min(grid_area.max.x));
        if z > a {
            gp.rect_filled(Rect::from_min_max(pos2(a, grid_area.min.y), pos2(z, rect.max.y)), CornerRadius::ZERO, shade_out);
            np0.rect_filled(Rect::from_min_max(pos2(a, nums.min.y), pos2(z, nums.max.y)), CornerRadius::ZERO, shade_out);
        }
    }
    for b in [w0, w1] {
        let x = x_of(b).round() + 0.5;
        if x > grid_area.min.x && x < grid_area.max.x {
            gp.line_segment([pos2(x, marker.min.y), pos2(x, rect.max.y)], Stroke::new(1.0, gray(0x91)));
        }
    }
    // Rubber-band time range (Ctrl+D duplicates it with its notes).
    // Live 12: a light-blue bar at the bottom of the scrub strip with grey pin
    // handles at both ends; the status bar reads "Time Selection …".
    if let Some((a, b)) = app.pr_time_sel {
        let (xa, xb) = (x_of(a).round(), x_of(b).round());
        let sp = ui.painter_at(ruler);
        sp.rect_filled(Rect::from_min_max(pos2(xa, ruler.max.y - 4.0), pos2(xb, ruler.max.y)), CornerRadius::ZERO, SEL_BLUE);
        for x in [xa, xb] {
            let pin = Rect::from_center_size(pos2(x, ruler.min.y + 5.0), vec2(7.0, 8.0));
            sp.rect_filled(pin, CornerRadius::same(1), gray(0x91));
            sp.rect_stroke(pin, CornerRadius::same(1), Stroke::new(1.0, gray(0x5d)), egui::StrokeKind::Inside);
            sp.line_segment([pos2(x + 0.5, pin.max.y), pos2(x + 0.5, ruler.max.y)], Stroke::new(1.0, gray(0x91)));
        }
        if app.focus_detail {
            let bpb_u = app.project.beats_per_bar.max(1);
            let f = |v: f64, l: bool| super::topbar::bbs(v, bpb_u, l).replace(' ', "");
            app.status_hover = Some(format!("Time Selection  Start: {}  End: {}  Length: {}", f(a, false), f(b, false), f(b - a, true)));
        }
    }

    // Notes (clipped to the scrolling area).
    let np = ui.painter_at(grid_area);
    let vp = ui.painter_at(vel_area);
    let note_rect = |n: &Note| {
        let (x1, x2) = (x_of(n.start), x_of(n.start + n.length).max(x_of(n.start) + 3.0));
        let y = y_of(n.pitch as i32);
        Rect::from_min_max(pos2(x1, y + 1.0), pos2(x2, y + row_h - 1.0))
    };
    // Notes take the clip colour (Live).
    let note_col = rgb(clip.color);
    let mut hit: Option<(usize, bool)> = None; // (index, on resize edge)
    let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
    for (i, n) in notes.iter().enumerate() {
        let r = note_rect(n);
        let sel = app.note_sel.contains(&i);
        if r.max.y > grid_area.min.y && r.min.y < grid_area.max.y && r.max.x > grid_area.min.x && r.min.x < grid_area.max.x {
            // Clip colour (darker when softer) also outside the markers (Live),
            // 2 px black border (1 px on tiny notes), selected = 2 px #b0ddeb
            // inside a 1 px black edge; bold name when it fits.
            let fill = if n.muted { gray(0x30) } else { shade(note_col, 0.45 + 0.55 * n.velocity as f32 / 127.0) };
            let r = Rect::from_min_max(r.min - vec2(0.0, 1.0), r.max + vec2(0.0, 1.0));
            np.rect_filled(r, CornerRadius::ZERO, fill);
            let bw = if r.width() >= 8.0 && r.height() >= 7.0 { 2.0 } else { 1.0 };
            if sel {
                np.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
                np.rect_stroke(r.shrink(1.0), CornerRadius::ZERO, Stroke::new(bw, SEL_BLUE), egui::StrokeKind::Inside);
            } else {
                np.rect_stroke(r, CornerRadius::ZERO, Stroke::new(bw, Color32::BLACK), egui::StrokeKind::Inside);
            }
            if row_h >= 9.0 && r.width() > 18.0 && drums.is_none() {
                np.with_clip_rect(r.shrink(1.0).intersect(grid_area)).text(
                    pos2(r.min.x + 4.0, r.center().y),
                    Align2::LEFT_CENTER,
                    note_name(n.pitch),
                    bold(FS),
                    if n.muted { gray(0x75) } else { text_on(fill) },
                );
            }
            // Note expression (MPE) pitch curve over the note.
            if let Some(e) = clip.note_mpe(n)
                && e.pitch.len() > 1
            {
                let yc = r.center().y;
                let line: Vec<egui::Pos2> = e.pitch.iter().map(|&(t, v)| pos2(x_of(n.start + t), yc - v * row_h)).collect();
                np.add(egui::Shape::line(line, Stroke::new(1.0, Color32::from_rgb(120, 200, 255))));
            }
            if let Some(pt) = pointer
                && grid_area.contains(pt)
                && r.contains(pt)
            {
                hit = Some((i, pt.x > r.max.x - 5.0));
            }
        }
    }
    // Velocity lane: tallest stems first so every overlapping (chord) stem keeps a
    // visible head; selected stems are drawn last on top.
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| (app.note_sel.contains(&i), std::cmp::Reverse(notes[i].velocity)));
    for i in order {
        let n = &notes[i];
        let x = x_of(n.start) + 1.0;
        if x < vel_area.min.x - 4.0 || x > vel_area.max.x + 4.0 {
            continue;
        }
        let sel = app.note_sel.contains(&i);
        let vh = vel_area.height() * n.velocity as f32 / 127.0;
        // Live 12 (hn-piano-roll-velocity): a small head filled with the note
        // colour + a dark line over the note's length (selected: blue).
        let (head, line) = if sel {
            (SEL_BLUE, SEL_BLUE)
        } else if n.muted {
            (gray(0x5d), gray(0x2a))
        } else {
            (note_col, Color32::from_rgb(41, 40, 35))
        };
        let top = pos2(x + 1.0, (vel_area.max.y - vh).round() + 0.5);
        let x_end = (x_of(n.start + n.length) - 1.0).max(x + 4.0);
        vp.line_segment([pos2(top.x + 2.0, top.y), pos2(x_end, top.y)], Stroke::new(1.0, line));
        vp.circle_filled(top, 2.2, head);
        vp.circle_stroke(top, 2.2, Stroke::new(0.8, Color32::from_rgb(44, 43, 39)));
    }
    // Velocity scale (1 / 64 / 127) in the key column.
    let sp = ui.painter_at(Rect::from_min_max(pos2(rect.min.x, vel_area.min.y - 4.0), pos2(vel_area.min.x, rect.max.y)));
    sp.rect_filled(
        Rect::from_min_max(pos2(rect.min.x, grid_area.max.y), pos2(vel_area.min.x - 2.0, rect.max.y)),
        CornerRadius::ZERO,
        panel(),
    );
    // Envelope / MPE lanes draw their own scale.
    let vel_scale: &[u8] = if app.cv.tab == super::clip::Tab::Main && lane_on { &[127, 64, 1] } else { &[] };
    for &v in vel_scale {
        let y = vel_area.max.y - vel_area.height() * v as f32 / 127.0;
        sp.line_segment([pos2(vel_area.min.x - 6.0, y), pos2(vel_area.min.x - 2.0, y)], Stroke::new(1.0, gray(120)));
        sp.text(pos2(vel_area.min.x - 8.0, y), Align2::RIGHT_CENTER, v.to_string(), super::theme::font(8.0), gray(150));
        if v == 64 {
            vp.line_segment([pos2(vel_area.min.x, y), pos2(vel_area.max.x, y)], Stroke::new(1.0, Color32::from_white_alpha(10)));
        }
    }
    if has_bar {
        velocity_bar(app, ui, bar, notes);
    }
    // Splitter between the note grid and the velocity lane.
    let split = Rect::from_min_max(pos2(rect.min.x, grid_area.max.y - 3.0), pos2(rect.max.x, grid_area.max.y + 4.0));
    let sr = ui.interact(split, ui.scope_id().with("vel_split"), if lane_on { Sense::drag() } else { Sense::hover() });
    p.line_segment(
        [pos2(rect.min.x, grid_area.max.y + 1.0), pos2(rect.max.x, grid_area.max.y + 1.0)],
        Stroke::new(if sr.hovered() || sr.dragged() { 2.0 } else { 1.0 }, gray(if sr.hovered() || sr.dragged() { 140 } else { 70 })),
    );
    if lane_on && (sr.hovered() || sr.dragged()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if sr.dragged()
        && let Some(pt) = sr.interact_pointer_pos()
    {
        app.pr_vel_h = (rect.max.y - pt.y).clamp(24.0, (rect.height() * 0.6).max(24.0));
    }
    // Piano keys: press / glide to play the pitch (held until release).
    let keys = Rect::from_min_max(pos2(rect.min.x, grid_area.min.y), pos2(rect.min.x + KEYS_W - 2.0, grid_area.max.y));
    let kr = ui.interact(keys, ui.scope_id().with("pr_keys"), Sense::click_and_drag());
    let held_id = ui.scope_id().with("pr_key_held");
    let held: Option<u8> = ui.data(|d| d.get_temp(held_id)).flatten();
    if kr.hovered() || kr.is_pointer_button_down_on() {
        super::cursors::set(ui.ctx(), super::cursors::Cur::Speaker);
    }
    let want = if kr.is_pointer_button_down_on() {
        ui.ctx().pointer_latest_pos().filter(|pt| keys.contains(*pt)).map(|pt| pitch_at(pt.y).clamp(0, 127) as u8).or(held)
    } else {
        None
    };
    if want != held {
        if let Some(rt) = app.rt(track) {
            if let Some(h) = held {
                app.send_now(daw_engine::Command::NoteOff { track: rt, pitch: h });
            }
            if let Some(w) = want {
                app.send_now(daw_engine::Command::NoteOn { track: rt, pitch: w, velocity: 100 });
            }
        }
        ui.data_mut(|d| d.insert_temp(held_id, want));
    }
    if let Some(h) = want {
        p.rect_filled(
            Rect::from_min_max(pos2(keys.min.x, y_of(h as i32)), pos2(keys.max.x, y_of(h as i32) + row_h - 1.0)),
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(255, 180, 60, 150),
        );
    }
    // Headphones (top-left, above the keys): preview notes while editing.
    let hp = Rect::from_min_max(pos2(rect.min.x + NAMES_W + 2.0, ruler.min.y + 1.0), pos2(rect.min.x + KEYS_W - 3.0, ruler.max.y - 1.0));
    p.rect_filled(hp.expand(1.0), CornerRadius::same(2), gray(0x11));
    let hr = ui.interact(hp, ui.scope_id().with("pr_audition"), Sense::click());
    p.rect_filled(
        hp,
        CornerRadius::same(2),
        if app.pr_audition {
            ORANGE
        } else if hr.hovered() {
            gray(0x2a)
        } else {
            gray(0x1e)
        },
    );
    draw_headphones(&p, hp.center(), if app.pr_audition { Color32::BLACK } else { gray(0x8c) });
    if hr.on_hover_text("Preview notes while placing / moving them").clicked() {
        app.pr_audition = !app.pr_audition;
    }
    if let Some((_, true)) = hit {
        super::cursors::set(ui.ctx(), super::cursors::Cur::DragRightEdge);
    }

    // Interaction
    let drag_id = ui.scope_id().with("note_drag");
    let in_vel = |pt: egui::Pos2| vel_area.contains(pt);
    // Ruler click: move the song playhead (arrangement clips map to song time).
    let arr_start = match cref {
        ClipRef::Arr { track, index } => app.project.tracks.get(track).and_then(|t| t.arrangement.get(index)).map(|a| a.start),
        ClipRef::Slot { .. } => None,
    };
    if resp.clicked()
        && let Some(pt) = resp.interact_pointer_pos()
        && ruler.contains(pt)
    {
        if let Some(s0) = arr_start {
            app.set_playhead(s0 + (snap_round(beat_at(pt.x)) - clip.start).clamp(0.0, len));
        }
        return;
    }
    if resp.double_clicked()
        && let Some(pt) = resp.interact_pointer_pos()
        && grid_area.contains(pt)
    {
        match hit {
            Some((i, _)) => {
                app.note_sel = vec![i];
                app.delete_selected_notes();
            }
            None => {
                // Length / velocity of the last edited note become the defaults.
                let pitch = pitch_at(pt.y).clamp(0, 127) as u8;
                let length = if app.pr_note_len > 0.0 { app.pr_note_len } else { grid.min(len) };
                let velocity = app.pr_note_vel.clamp(1, 127);
                let start = snap(beat_at(pt.x));
                app.edit(|pr| {
                    if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(pr, cref) {
                        notes.push(Note { start, length, pitch, velocity, muted: false, mpe: 0, vel_dev: 0 });
                        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                    }
                });
                app.note_sel.clear();
                app.pr_time_sel = None;
                if app.pr_audition {
                    app.audition_note(track, pitch, velocity);
                }
            }
        }
        return;
    }
    if resp.clicked()
        && let Some(pt) = resp.interact_pointer_pos()
    {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        match hit {
            Some((i, _)) => {
                if add {
                    if let Some(k) = app.note_sel.iter().position(|&s| s == i) {
                        app.note_sel.remove(k);
                    } else {
                        app.note_sel.push(i);
                    }
                } else {
                    app.note_sel = vec![i];
                }
                let n = notes[i];
                app.pr_time_sel = None;
                if app.pr_audition {
                    app.audition_note(track, n.pitch, n.velocity);
                }
            }
            None if !in_vel(pt) => {
                app.note_sel.clear();
                app.pr_time_sel = None;
            }
            None => {}
        }
    }
    // Right-click: select the note under the pointer, then the context menu.
    if resp.secondary_clicked()
        && let Some((i, _)) = hit
        && !app.note_sel.contains(&i)
    {
        app.note_sel = vec![i];
    }
    resp.context_menu(|ui| {
        let any = !app.note_sel.is_empty();
        if ui.add_enabled(any, egui::Button::new("Delete                   Del")).clicked() {
            app.delete_selected_notes();
            ui.close();
        }
        if ui.add_enabled(any, egui::Button::new("Duplicate               Ctrl+D")).clicked() {
            app.duplicate_selected_notes();
            ui.close();
        }
        if ui.add_enabled(any, egui::Button::new("Activate / Deactivate   0")).clicked() {
            app.focus_detail = true;
            app.toggle_disable();
            ui.close();
        }
        if ui.button("Select All              Ctrl+A").clicked() {
            app.note_sel = (0..notes.len()).collect();
            ui.close();
        }
        ui.separator();
        super::widgets::grid_menu(ui, &mut app.pr_grid, f64::INFINITY);
    });
    // Note-grid press deselects the loop brace (arrows go back to notes).
    if (resp.drag_started() || resp.clicked()) && ui.input(|i| i.pointer.press_origin()).is_some_and(|p| grid_area.contains(p)) {
        app.cv.brace_sel = false;
    }
    if resp.drag_started()
        && let Some(pt) = ui.input(|i| i.pointer.press_origin())
    {
        // Re-hit-test at the press point (the pointer has moved past the drag threshold).
        let hit = grid_area
            .contains(pt)
            .then(|| {
                notes.iter().enumerate().rev().find_map(|(i, n)| {
                    let r = note_rect(n);
                    r.contains(pt).then_some((i, pt.x > r.max.x - 5.0))
                })
            })
            .flatten();
        let mode = if ruler.contains(pt) {
            Some(NoteDrag::Ruler)
        } else if in_vel(pt) {
            Some(NoteDrag::Velocity)
        } else if grid_area.contains(pt) {
            match hit {
                Some((i, edge)) => {
                    if !app.note_sel.contains(&i) {
                        app.note_sel = vec![i];
                    }
                    Some(if edge {
                        NoteDrag::Resize { grab: i }
                    } else {
                        NoteDrag::Move {
                            grab_beat: beat_at(pt.x),
                            grab_pitch: pitch_at(pt.y),
                            orig: Some((notes[i].start, notes[i].pitch)),
                        }
                    })
                }
                None => Some(NoteDrag::Marquee),
            }
        } else {
            None
        };
        if let Some(m) = mode {
            if matches!(m, NoteDrag::Move { .. } | NoteDrag::Resize { .. } | NoteDrag::Velocity) {
                app.begin_gesture();
            }
            let base = if ui.input(|i| i.modifiers.shift || i.modifiers.command) { app.note_sel.clone() } else { Vec::new() };
            let mut snapshot = notes.to_vec();
            // Ctrl+drag on a note: move copies, leaving the originals in place.
            if matches!(m, NoteDrag::Move { .. }) && ui.input(|i| i.modifiers.command) {
                let first = snapshot.len();
                let copies: Vec<Note> = app.note_sel.iter().filter_map(|&i| snapshot.get(i).copied()).collect();
                snapshot.extend(copies);
                app.note_sel = (first..snapshot.len()).collect();
                if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
                    *notes = snapshot.clone();
                }
            }
            if matches!(m, NoteDrag::Marquee) && base.is_empty() {
                app.pr_time_sel = None;
            }
            // Snapshot of the notes at drag start lets moves be computed from absolute deltas.
            ui.data_mut(|d| {
                d.insert_temp(drag_id.with("view"), (view_lo, view_hi));
                d.insert_temp(drag_id, m);
                d.insert_temp(drag_id.with("orig"), snapshot);
                d.insert_temp(drag_id.with("base"), base);
            });
        }
    }
    if resp.dragged()
        && let Some(mode) = ui.data(|d| d.get_temp::<NoteDrag>(drag_id))
        && let Some(orig) = ui.data(|d| d.get_temp::<Vec<Note>>(drag_id.with("orig")))
        && let Some(pt) = resp.interact_pointer_pos()
    {
        let sel = app.note_sel.clone();
        let mut new = orig.clone();
        let mut write = true;
        match mode {
            NoteDrag::Move { grab_beat, grab_pitch, orig: anchor } => {
                let (a_start, _) = anchor.unwrap_or((0.0, 0));
                let raw = a_start + beat_at(pt.x) - grab_beat;
                let target = if free { raw } else { snap(raw + grid * 0.5) };
                let db = target - a_start;
                let dp = pitch_at(pt.y) - grab_pitch;
                // Notes may leave the clip on either side (kept, silent: Live).
                for &i in &sel {
                    if let Some(n) = new.get_mut(i) {
                        n.start = orig[i].start + db;
                        n.pitch = (orig[i].pitch as i32 + dp).clamp(0, 127) as u8;
                    }
                }
                // Preview the new pitch of the grabbed note when it changes.
                if let Some((_, p0)) = anchor {
                    let np = (p0 as i32 + dp).clamp(0, 127) as u8;
                    let key = drag_id.with("last_pitch");
                    if ui.data(|d| d.get_temp::<u8>(key)) != Some(np) {
                        ui.data_mut(|d| d.insert_temp(key, np));
                        if dp != 0 && app.pr_audition {
                            let v = sel.first().and_then(|&i| orig.get(i)).map_or(100, |n| n.velocity);
                            app.audition_note(track, np, v);
                        }
                    }
                }
            }
            NoteDrag::Resize { grab } => {
                let min = if free { 1.0 / 128.0 } else { grid.min(0.25) / 4.0 };
                if let Some(g) = orig.get(grab) {
                    // The grabbed note's end follows the (snapped) pointer; the rest shift by the same delta.
                    let end = snap_round(beat_at(pt.x)).max(g.start + min);
                    let delta = end - (g.start + g.length);
                    for &i in &sel {
                        if let (Some(n), Some(o)) = (new.get_mut(i), orig.get(i)) {
                            n.length = (o.length + delta).max(min).max(1.0 / 128.0);
                        }
                    }
                }
                if let Some(n) = sel.first().and_then(|&i| new.get(i)) {
                    app.pr_note_len = n.length;
                }
            }
            NoteDrag::Velocity => {
                let v = (((vel_area.max.y - pt.y) / vel_area.height()) * 127.0).round().clamp(1.0, 127.0) as u8;
                app.pr_note_vel = v;
                // Value readout next to the pointer while dragging.
                let tp = ui.painter();
                let lr = Rect::from_center_size(pt + vec2(18.0, -12.0), vec2(28.0, 15.0));
                tp.rect_filled(lr, CornerRadius::same(2), Color32::from_black_alpha(210));
                tp.text(lr.center(), Align2::CENTER_CENTER, v.to_string(), super::theme::font(10.0), Color32::WHITE);
                let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(pt);
                let b = beat_at(press.x);
                // Selected notes, else every note starting at the nearest start (a chord).
                let targets: Vec<usize> = if sel.is_empty() {
                    let nearest = orig.iter().map(|n| n.start).min_by(|a, c| (a - b).abs().total_cmp(&(c - b).abs()));
                    match nearest {
                        Some(s) => (0..orig.len()).filter(|&i| (orig[i].start - s).abs() < 1e-9).collect(),
                        None => Vec::new(),
                    }
                } else {
                    sel.clone()
                };
                for i in targets {
                    if let Some(n) = new.get_mut(i) {
                        n.velocity = v;
                    }
                }
            }
            NoteDrag::Marquee => {
                write = false;
                let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(pt);
                let band = Rect::from_two_pos(start, pt);
                let mut s: Vec<usize> = ui.data(|d| d.get_temp::<Vec<usize>>(drag_id.with("base"))).unwrap_or_default();
                for (i, n) in orig.iter().enumerate() {
                    if note_rect(n).intersects(band) && !s.contains(&i) {
                        s.push(i);
                    }
                }
                app.note_sel = s;
                let (a, b) = (beat_at(band.min.x).max(view_lo), beat_at(band.max.x).max(view_lo));
                app.pr_time_sel =
                    (b - a > 1e-6).then_some((snap_round(a), snap_round(b).max(snap_round(a) + if free { b - a } else { grid })));
                // Live 12: dashed white rectangle, no fill.
                let b = band.shrink(0.5);
                let pts = [b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom(), b.left_top()];
                np.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, gray(0xd0)), 4.0, 3.0));
            }
            NoteDrag::Ruler => {
                write = false;
                if let Some(s0) = arr_start {
                    app.set_playhead(s0 + (snap_round(beat_at(pt.x)) - clip.start).clamp(0.0, len));
                }
            }
        }
        // Near / past an edge the view runs on (Live): notes can be dragged
        // out into the space beyond the clip on both sides. Zoomed in it
        // scrolls; fully zoomed out the view widens (zooms out).
        if matches!(mode, NoteDrag::Move { .. } | NoteDrag::Resize { .. } | NoteDrag::Marquee) {
            let step = 6.0 / ppb;
            let h = app.pr_hold.get_or_insert((view_lo, view_hi));
            if pt.x > grid_area.max.x - 14.0 {
                h.1 = (h.1.max(scroll + visible) + step).min(MAX_VIEW_BEATS);
                if app.pr_zoom > 0.0 {
                    app.pr_scroll += step;
                }
                ui.ctx().request_repaint();
            } else if pt.x < grid_area.min.x + 14.0 {
                h.0 = (h.0.min(scroll) - step).max(-MAX_VIEW_BEATS);
                if app.pr_zoom > 0.0 {
                    app.pr_scroll -= step;
                }
                ui.ctx().request_repaint();
            }
        }
        if write && let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
            *notes = new;
        }
    }
    if resp.drag_stopped()
        && let Some(mode) = ui.data(|d| d.get_temp::<NoteDrag>(drag_id))
    {
        // The zoom-out limit keeps where notes were and now are, without the
        // overshoot of the edge auto-scroll (Live: zoom stays until you zoom in).
        let pre: Option<(f64, f64)> = ui.data(|d| d.get_temp(drag_id.with("view")));
        if let Some((a, b)) = pre
            && matches!(mode, NoteDrag::Move { .. } | NoteDrag::Resize { .. } | NoteDrag::Marquee)
            && let Some(c) = app.clip(cref).cloned()
            && let ClipContent::Midi { notes: now } = &c.content
        {
            let (lo, hi) = content_span(&c, now);
            app.pr_hold = Some((a.min(lo), b.max(hi)));
        }
        ui.data_mut(|d| {
            d.remove::<(f64, f64)>(drag_id.with("view"));
            d.remove::<u8>(drag_id.with("last_pitch"));
            d.remove::<NoteDrag>(drag_id);
            d.remove::<Vec<Note>>(drag_id.with("orig"));
            d.remove::<Vec<usize>>(drag_id.with("base"));
        });
        if matches!(mode, NoteDrag::Move { .. } | NoteDrag::Resize { .. } | NoteDrag::Velocity) {
            // Re-sort while keeping the selection on the same notes.
            if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
                let selected: Vec<Note> = app.note_sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
                notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                app.note_sel = selected.iter().filter_map(|s| notes.iter().position(|n| n == s)).collect();
            }
            app.needs_compile = true;
        }
    }
    // Marker strip, envelope / MPE lane, loop brace keys, playhead (content position).
    super::clip::marker_strip(app, ui, marker, cref, clip, &x_of, &beat_at, &snap_round);
    match app.cv.tab {
        super::clip::Tab::Envelopes => super::clip::env_lane(app, ui, vel_area, cref, clip, &x_of, &beat_at, &snap_round),
        super::clip::Tab::Mpe => super::clip::mpe_lane(app, ui, vel_area, cref, clip, notes, &x_of, &beat_at),
        super::clip::Tab::Main => {}
    }
    super::clip::brace_keys(app, ui, cref, grid);
    let _ = track;
    let pos = super::clip::play_pos(app, cref);
    if app.cv.follow
        && let Some(pp) = pos
        && (pp < app.pr_scroll || pp > app.pr_scroll + visible)
    {
        app.pr_scroll = pp;
    }
    if let Some(pos) = pos {
        let x = x_of(pos);
        gp.line_segment([pos2(x, ruler.min.y), pos2(x, rect.max.y)], Stroke::new(1.5, Color32::WHITE));
    }
}

// ============================ Devices ============================

/// Live 12 device view (measured at 1920 px): the panel fills the detail area,
/// a `#18` double grip sits in the left window margin, devices run left to right
/// and the rest of the chain is the centred "Drop Audio Effects Here" zone.
fn device_view(app: &mut DawApp, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    ui.allocate_rect(rect, Sense::hover());
    // Grip: two 2 px bars at panel.left-7 / -4, inset 40 px, notch 11 px below the top.
    let gp = ui.painter().with_clip_rect(rect.expand2(vec2(9.0, 0.0)));
    let (gy0, gy1) = (rect.top() + 40.0, rect.bottom() - 40.0);
    for dx in [-7.0, -4.0] {
        let x = rect.left() + dx;
        gp.rect_filled(Rect::from_min_max(pos2(x, gy0), pos2(x + 2.0, gy1)), 0.0, gray(0x18));
        gp.rect_filled(Rect::from_min_max(pos2(x, gy0 + 11.0), pos2(x + 2.0, gy0 + 12.0)), 0.0, panel());
    }
    let t = app.selected_track;
    let midi =
        !app.master_selected && app.selected_return.is_none() && app.project.tracks.get(t).is_some_and(|tr| tr.kind == TrackKind::Midi);
    let zone = ui.interact(rect, ui.make_persistent_id(("dev_zone", t, app.master_selected, app.selected_return)), Sense::hover());
    let mut fx = rect;
    let mut inst_drop_ok = false;
    if midi {
        inst_drop_ok = true;
        match app.project.tracks[t].instrument {
            InstrumentDef::None => {
                // Empty MIDI track: the whole chain takes an instrument.
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, "Drop an Instrument or Sample Here", font(FS), text());
                fx = Rect::NOTHING;
            }
            _ => {
                let used = instrument_device(app, ui, rect);
                fx = Rect::from_min_max(pos2(used.right() + 1.0, rect.top()), rect.max);
            }
        }
    }
    if fx.is_positive() {
        ui.painter().with_clip_rect(fx).text(fx.center(), Align2::CENTER_CENTER, "Drop Audio Effects Here", font(FS), text());
    }
    // Drag & drop: instruments / samples replace the device (audio effects: Phase 6).
    let hover = zone.dnd_hover_payload::<DragItem>();
    if inst_drop_ok && hover.as_deref().is_some_and(|d| super::instrument_for(d).is_some() || matches!(d, DragItem::Sample { .. })) {
        ui.painter().rect_stroke(rect.shrink(1.0), CornerRadius::same(3), Stroke::new(1.0, focus()), egui::StrokeKind::Inside);
    }
    if !inst_drop_ok {
        return;
    }
    if let Some(item) = zone.dnd_release_payload::<DragItem>() {
        let inst = app.project.tracks[t].instrument.clone();
        if let Some(def) = match &*item {
            // Sample onto a sampler: keep its params; onto anything else: new Sampler.
            DragItem::Sample { key, .. } => match &inst {
                InstrumentDef::Sampler { params, .. } => Some(InstrumentDef::Sampler { sample: Some(key.clone()), params: *params }),
                InstrumentDef::Drums { .. } => None, // pads take samples individually
                _ => super::instrument_for(&item),
            },
            other => super::instrument_for(other),
        } {
            app.set_instrument(t, def);
        }
    }
}

/// Draws the selected MIDI track's instrument as a Live device at the left of
/// `area`; returns the device rect.
fn instrument_device(app: &mut DawApp, ui: &mut egui::Ui, area: Rect) -> Rect {
    let t = app.selected_track;
    let mut inst = app.project.tracks[t].instrument.clone();
    let w = match inst {
        InstrumentDef::Synth(_) => 880.0_f32,
        InstrumentDef::Sampler { .. } => 470.0,
        _ => 640.0,
    };
    let dev = Rect::from_min_size(area.min, vec2(w.min(area.width() - 120.0).max(200.0), area.height()));
    let p = ui.painter().with_clip_rect(dev);
    // Title strip: activator dot + name; 1 px chain separator on the right.
    let title = Rect::from_min_size(dev.min, vec2(dev.width(), 17.0));
    p.rect_filled(title, CornerRadius { nw: 3, ne: 0, sw: 0, se: 0 }, gray(0x46));
    p.circle_filled(title.left_center() + vec2(9.0, 0.0), 4.5, ORANGE);
    p.text(title.left_center() + vec2(19.0, 0.0), Align2::LEFT_CENTER, inst.label(), bold(FS), text());
    ui.painter().rect_filled(Rect::from_min_max(pos2(dev.right(), dev.top()), pos2(dev.right() + 1.0, dev.bottom())), 0.0, sep());
    let body = Rect::from_min_max(pos2(dev.left() + 6.0, title.bottom() + 4.0), dev.max - vec2(6.0, 4.0));
    let mut ed = Edit::default();
    let mut audition = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt(("dev", t)));
    child.set_clip_rect(body);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| match &mut inst {
        InstrumentDef::None => {}
        InstrumentDef::Synth(s) => synth_ui(ui, s, &mut ed),
        InstrumentDef::Sampler { sample, params } => {
            ui.horizontal(|ui| {
                ui.label("Sample:");
                let name = sample
                    .as_deref()
                    .map(|s| s.rsplit(['/', '\\', ':']).next().unwrap_or(s).to_string())
                    .unwrap_or_else(|| "(drop a sample here)".into());
                ui.label(egui::RichText::new(name).color(ACCENT));
            });
            ui.horizontal(|ui| {
                ui.label("Root");
                ed.drag(ui.add(egui::DragValue::new(&mut params.root_note).range(0..=127).custom_formatter(|v, _| note_name(v as u8))));
            });
            ed.slider(ui, "Tune", &mut params.tune_semitones, -24.0..=24.0, false);
            ed.slider(ui, "Gain", &mut params.gain, 0.0..=2.0, false);
            adsr_ui(ui, "Amp", &mut params.amp, &mut ed);
        }
        InstrumentDef::Drums { pads } => audition = drums_ui(ui, pads, &mut ed),
    });
    if let Some(a) = audition {
        app.audition_note(t, a, 110);
    }
    if let Some((pad, key)) = ed.pad_drop.take() {
        if let InstrumentDef::Drums { pads } = &mut inst {
            while pads.len() <= pad {
                pads.push(PadDef { name: format!("Pad {}", pads.len() + 1), gain: 0.8, ..Default::default() });
            }
            pads[pad].name = key.1;
            pads[pad].sample = Some(key.0);
        }
        ed.gesture = true;
        ed.changed = true;
    }
    if ed.gesture {
        app.begin_gesture();
    }
    if ed.changed {
        app.project.tracks[t].instrument = inst;
        app.needs_compile = true;
        app.dirty = true;
    }
    dev
}

#[derive(Default)]
struct Edit {
    gesture: bool,
    changed: bool,
    pad_drop: Option<(usize, (String, String))>,
}

impl Edit {
    fn drag(&mut self, r: egui::Response) {
        self.gesture |= r.drag_started() || r.gained_focus() || (r.clicked() && r.changed());
        self.changed |= r.changed();
    }

    fn slider(&mut self, ui: &mut egui::Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, log: bool) {
        let r = ui.add(egui::Slider::new(v, range).text(label).logarithmic(log).max_decimals(2));
        self.drag(r);
    }
}

fn synth_ui(ui: &mut egui::Ui, s: &mut daw_engine::SynthParams, ed: &mut Edit) {
    ui.horizontal(|ui| {
        ui.label("Preset");
        egui::ComboBox::from_id_salt("preset").selected_text("Load…").show_ui(ui, |ui| {
            for p in daw_engine::SynthParams::PRESETS {
                if ui.selectable_label(false, p).clicked() {
                    *s = daw_engine::SynthParams::preset(p);
                    ed.gesture = true;
                    ed.changed = true;
                }
            }
        });
        wave_combo(ui, "Osc 1", &mut s.osc1, ed);
        wave_combo(ui, "Osc 2", &mut s.osc2, ed);
    });
    // Four columns keep every control inside Live's fixed device height.
    ui.columns(4, |c| {
        c[0].label(egui::RichText::new("OSCILLATORS").small().color(Color32::GRAY));
        ed.slider(&mut c[0], "Osc2 semi", &mut s.osc2_semitones, -24.0..=24.0, false);
        ed.slider(&mut c[0], "Detune ct", &mut s.osc2_detune_cents, -50.0..=50.0, false);
        ed.slider(&mut c[0], "Mix", &mut s.osc_mix, 0.0..=1.0, false);
        ed.slider(&mut c[0], "Gain", &mut s.gain, 0.0..=1.5, false);
        c[1].label(egui::RichText::new("FILTER").small().color(Color32::GRAY));
        ed.slider(&mut c[1], "Cutoff Hz", &mut s.cutoff_hz, 30.0..=18000.0, true);
        ed.slider(&mut c[1], "Resonance", &mut s.resonance, 0.0..=1.0, false);
        ed.slider(&mut c[1], "Env oct", &mut s.filter_env_octaves, -4.0..=8.0, false);
        adsr_ui(&mut c[2], "FILTER ENV", &mut s.filter, ed);
        adsr_ui(&mut c[3], "AMP ENV", &mut s.amp, ed);
    });
}

fn wave_combo(ui: &mut egui::Ui, label: &str, w: &mut Wave, ed: &mut Edit) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label).selected_text(format!("{w:?}")).width(80.0).show_ui(ui, |ui| {
        for v in [Wave::Sine, Wave::Saw, Wave::Square, Wave::Triangle] {
            if ui.selectable_label(*w == v, format!("{v:?}")).clicked() && *w != v {
                *w = v;
                ed.gesture = true;
                ed.changed = true;
            }
        }
    });
}

fn adsr_ui(ui: &mut egui::Ui, label: &str, a: &mut daw_engine::AdsrParams, ed: &mut Edit) {
    ui.label(egui::RichText::new(label).small());
    ed.slider(ui, "A s", &mut a.attack, 0.0005..=5.0, true);
    ed.slider(ui, "D s", &mut a.decay, 0.001..=5.0, true);
    ed.slider(ui, "S", &mut a.sustain, 0.0..=1.0, false);
    ed.slider(ui, "R s", &mut a.release, 0.001..=8.0, true);
}

/// 4×4 pad grid (bottom-left = C1 like hardware). Returns a pad to audition.
fn drums_ui(ui: &mut egui::Ui, pads: &mut [PadDef], ed: &mut Edit) -> Option<u8> {
    let sel_id = ui.scope_id().with("pad_sel");
    let mut sel: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(0);
    let mut audition = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for row in (0..4).rev() {
                ui.horizontal(|ui| {
                    for col in 0..4 {
                        let i = row * 4 + col;
                        if i >= DRUM_PADS {
                            continue;
                        }
                        let (r, resp) = ui.allocate_exact_size(vec2(78.0, 32.0), Sense::click());
                        let has = pads.get(i).is_some_and(|p| p.sample.is_some());
                        let bg = if i == sel {
                            ACCENT
                        } else if has {
                            gray(0x50)
                        } else {
                            gray(0x3e)
                        };
                        ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
                        ui.painter().rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
                        let name = pads.get(i).map_or(String::new(), |p| p.name.clone());
                        ui.painter().with_clip_rect(r).text(
                            r.left_top() + vec2(4.0, 3.0),
                            Align2::LEFT_TOP,
                            &name,
                            font(FS),
                            if i == sel { Color32::BLACK } else { text() },
                        );
                        ui.painter().text(
                            r.right_bottom() - vec2(3.0, 2.0),
                            Align2::RIGHT_BOTTOM,
                            note_name(DRUM_BASE_NOTE + i as u8),
                            super::theme::font(7.0),
                            gray(150),
                        );
                        if resp.clicked() {
                            sel = i;
                            audition = Some(DRUM_BASE_NOTE + i as u8);
                        }
                        if resp.dnd_hover_payload::<DragItem>().is_some() {
                            ui.painter().rect_stroke(r, CornerRadius::same(3), Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                        }
                        if let Some(item) = resp.dnd_release_payload::<DragItem>()
                            && let DragItem::Sample { key, name } = &*item
                        {
                            ed.pad_drop = Some((i, (key.clone(), name.clone())));
                            sel = i;
                        }
                    }
                });
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            if let Some(pad) = pads.get_mut(sel) {
                ui.label(egui::RichText::new(format!("Pad {} · {}", sel + 1, note_name(DRUM_BASE_NOTE + sel as u8))).strong());
                let r = ui.text_edit_singleline(&mut pad.name);
                ed.drag(r);
                ed.slider(ui, "Gain", &mut pad.gain, 0.0..=2.0, false);
                ed.slider(ui, "Tune", &mut pad.tune_semitones, -24.0..=24.0, false);
                let r = ui.add(egui::DragValue::new(&mut pad.choke).range(0..=8).prefix("Choke group "));
                ed.drag(r);
                ui.label(
                    egui::RichText::new(pad.sample.clone().unwrap_or_else(|| "(empty: drop a sample)".into())).small().color(Color32::GRAY),
                );
            } else {
                ui.label("Empty pad: drop a sample on it.");
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
    audition
}

/// Small headphones glyph (font-independent).
fn draw_headphones(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    let r = 4.5;
    let pts: Vec<egui::Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
            c + vec2(a.cos() * r, a.sin() * r + 1.0)
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
    for sx in [-1.0f32, 1.0] {
        p.rect_filled(Rect::from_center_size(c + vec2(sx * (r - 0.5), 2.5), vec2(2.5, 4.0)), CornerRadius::same(1), col);
    }
}

/// 0..127 value box with a teal level bar (Live's velocity bar fields).
/// Drag / double-click-to-type; returns the new value.
fn level_box(ui: &mut egui::Ui, r: Rect, id: &str, v: i32, lo: i32, hi: i32, teal: bool) -> Option<i32> {
    use super::cview::{Field, TEAL, value_box};
    let (resp, out) = value_box(ui, r, id, gray(0x1e), Field::Text(String::new()), 1.0, v.to_string(), |s| s.trim().parse::<f64>().ok());
    let editing = ui.data(|d| d.get_temp::<String>(ui.make_persistent_id(id).with("edit")).is_some());
    if !editing {
        let p = ui.painter();
        if teal && v > lo {
            let inner = r.shrink(1.0);
            let f = (v - lo) as f32 / (hi - lo).max(1) as f32;
            p.rect_filled(Rect::from_min_size(inner.min, vec2(inner.width() * f, inner.height())), 0.0, TEAL);
        }
        p.text(r.center(), Align2::CENTER_CENTER, v.to_string(), font(FS), text());
    }
    out.map(|d| (if resp.dragged() { v as f64 + d } else { d }).round().clamp(lo as f64, hi as f64) as i32)
}

/// Velocity bar under the piano roll (Live 12, hn-piano-roll-velocity):
/// lane toggle + ▾, then Randomize, Ramp and Deviation for the selected notes.
fn velocity_bar(app: &mut DawApp, ui: &mut egui::Ui, bar: Rect, notes: &[Note]) {
    let p = ui.painter_at(bar);
    p.rect_filled(bar, CornerRadius::ZERO, panel());
    p.line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, sep()));
    let x0 = bar.min.x;
    let cy = bar.center().y + 0.5;
    let at = |a: f32, b: f32, h: f32| Rect::from_min_max(pos2(x0 + a, cy - h / 2.0), pos2(x0 + b, cy + h / 2.0));
    // Orange toggle: shows / hides the lane (collapsed: dark with grey icon).
    let tr = at(3.0, 16.0, 13.0);
    let resp = ui.interact(tr, ui.make_persistent_id("vel_lane_btn"), Sense::click());
    let open = !app.pr_vel_collapsed;
    let (fill, edge, icon) = if open { (ORANGE, gray(0x11), Color32::from_rgb(131, 90, 46)) } else { (gray(0x11), gray(0x91), gray(0x91)) };
    p.rect_filled(tr, 0.0, edge);
    p.rect_filled(tr.shrink(1.0), 0.0, fill);
    // Icon: a velocity marker lying down (head + stem).
    let c = tr.center();
    p.rect_filled(Rect::from_center_size(pos2(c.x - 2.5, c.y), vec2(3.0, 3.0)), 0.0, icon);
    p.line_segment([pos2(c.x - 1.0, c.y), pos2(c.x + 4.0, c.y)], Stroke::new(1.0, icon));
    if resp.clicked() {
        app.pr_vel_collapsed = open;
    }
    resp.on_hover_text(if open { "Hide the velocity lane" } else { "Show the velocity lane" });
    // ▾: lane chooser (only Velocity exists).
    let dr = at(18.0, 28.0, 13.0);
    let dresp = ui.interact(dr, ui.make_persistent_id("vel_lane_menu"), Sense::click());
    let c = dr.center();
    p.add(egui::Shape::convex_polygon(
        vec![pos2(c.x - 3.5, c.y - 2.0), pos2(c.x + 3.5, c.y - 2.0), pos2(c.x, c.y + 2.0)],
        gray(if open { 0xb0 } else { 0x69 }),
        Stroke::NONE,
    ));
    super::menu::dropdown(&dresp, 120.0, |ui| {
        if super::menu::check_item(ui, "Velocity", "", open, true) {
            app.pr_vel_collapsed = open;
            ui.close();
        }
        super::menu::check_item(ui, "Chance", "", false, false);
    });
    p.line_segment([pos2(x0 + 32.5, bar.min.y + 1.0), pos2(x0 + 32.5, bar.max.y)], Stroke::new(1.0, sep()));
    if !open {
        return;
    }
    let sel: Vec<&Note> = app.note_sel.iter().filter_map(|&i| notes.get(i)).collect();
    let any = !sel.is_empty();
    let txt = |x: f32, s: &str| {
        p.text(pos2(x0 + x, cy), Align2::LEFT_CENTER, s, font(FS), if any { text() } else { gray(0x8a) });
    };
    txt(35.0, "Velocity");
    if super::cview::pill(ui, at(78.0, 142.0, 12.0), "vel_rand", "Randomize", any).clicked() {
        app.randomize_note_velocity(app.pr_rand_amt);
    }
    if let Some(v) = level_box(ui, at(146.0, 184.0, 13.0), "vel_rand_amt", app.pr_rand_amt as i32, 0, 127, true) {
        app.pr_rand_amt = v as u8;
    }
    p.line_segment([pos2(x0 + 189.5, cy - 6.0), pos2(x0 + 189.5, cy + 6.0)], Stroke::new(1.0, sep()));
    txt(195.0, "Ramp");
    let ramp = ui.interact(at(193.0, 224.0, 13.0), ui.make_persistent_id("vel_ramp"), if any { Sense::click() } else { Sense::hover() });
    if ramp.clicked() {
        app.ramp_note_velocity(app.pr_ramp.0, app.pr_ramp.1);
    }
    ramp.on_hover_text("Ramp the selected notes' velocities from the first to the second value");
    if let Some(v) = level_box(ui, at(226.0, 264.0, 13.0), "vel_ramp_a", app.pr_ramp.0 as i32, 1, 127, true) {
        app.pr_ramp.0 = v as u8;
    }
    if let Some(v) = level_box(ui, at(268.0, 306.0, 13.0), "vel_ramp_b", app.pr_ramp.1 as i32, 1, 127, false) {
        app.pr_ramp.1 = v as u8;
    }
    p.line_segment([pos2(x0 + 311.5, cy - 6.0), pos2(x0 + 311.5, cy + 6.0)], Stroke::new(1.0, sep()));
    txt(317.0, "Deviation");
    let dev = sel.first().map_or(0, |n| n.vel_dev as i32);
    if let Some(v) = level_box(ui, at(367.0, 425.0, 13.0), "vel_dev", dev, -127, 127, false)
        && any
    {
        app.set_note_deviation(v as i8);
    }
}
