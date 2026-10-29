//! Live 12 browser: search row, collapsed sidebar (Collections / Library /
//! Places), Content filter, "Name" list with 19 px rows, Tags and Preview
//! footer. Our content: instruments, factory sounds and the file system;
//! Live categories we don't have are greyed out. Items drag onto tracks,
//! slots, lanes or devices.

use super::theme::*;
use super::widgets::*;
use crate::app::{DawApp, DragItem};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use std::path::PathBuf;

const AUDIO_EXT: [&str; 10] = ["wav", "flac", "mp3", "ogg", "opus", "aif", "aiff", "m4a", "aac", "wave"];
const ROW: f32 = 19.0;
const SIDE_W: f32 = 27.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    All,
    Sounds,
    Drums,
    Instruments,
    Samples,
    Files,
    Other,
}

#[derive(Clone, Copy, PartialEq)]
enum Ico {
    All,
    Sounds,
    Drums,
    Instruments,
    AudioFx,
    MidiFx,
    Modulators,
    Max,
    Plugins,
    Clips,
    Samples,
    Grooves,
    Templates,
    Folder,
    Preset,
    Rack,
}

/// Library / Places sidebar entries: (icon, name, category; Other = greyed).
const LIB: [(Ico, &str, Cat); 13] = [
    (Ico::All, "All", Cat::All),
    (Ico::Sounds, "Sounds", Cat::Sounds),
    (Ico::Drums, "Drums", Cat::Drums),
    (Ico::Instruments, "Instruments", Cat::Instruments),
    (Ico::AudioFx, "Audio Effects", Cat::Other),
    (Ico::MidiFx, "MIDI Effects", Cat::Other),
    (Ico::Modulators, "Modulators", Cat::Other),
    (Ico::Max, "Max for Live", Cat::Other),
    (Ico::Plugins, "Plug-Ins", Cat::Other),
    (Ico::Clips, "Clips", Cat::Other),
    (Ico::Samples, "Samples", Cat::Samples),
    (Ico::Grooves, "Grooves", Cat::Other),
    (Ico::Templates, "Templates", Cat::Other),
];

const COLLECTIONS: [[u8; 3]; 7] = [
    [0xff, 0x05, 0x05],
    [0xff, 0x9e, 0x17],
    [0xff, 0xee, 0x34],
    [0x1a, 0xff, 0x9f],
    [0x00, 0x95, 0xff],
    [0xa6, 0x66, 0xff],
    [0x77, 0x77, 0x77],
];

/// Browser UI state (egui memory; not saved).
#[derive(Clone)]
struct State {
    cat: Cat,
    open: Vec<String>,
    selected: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self { cat: Cat::All, open: vec!["Instruments".into(), "Drum Rack".into(), "Synth".into()], selected: None }
    }
}

fn icon(p: &egui::Painter, ic: Ico, c: Pos2, col: Color32) {
    let s = Stroke::new(1.0, col);
    let r = Rect::from_center_size(c, vec2(15.0, 11.0));
    match ic {
        Ico::All => {
            for i in 0..4 {
                let x = r.left() + 2.0 + i as f32 * 3.0;
                p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], s);
            }
            p.line_segment([pos2(r.right() - 3.0, r.top()), pos2(r.right(), r.bottom())], s);
        }
        Ico::Sounds => {
            p.circle_stroke(pos2(r.left() + 4.0, r.bottom() - 2.0), 2.2, s);
            p.circle_stroke(pos2(r.right() - 3.0, r.bottom() - 3.0), 2.2, s);
            p.line_segment([pos2(r.left() + 6.2, r.bottom() - 2.0), pos2(r.left() + 6.2, r.top())], s);
            p.line_segment([pos2(r.right() - 0.8, r.bottom() - 3.0), pos2(r.right() - 0.8, r.top())], s);
            p.line_segment([pos2(r.left() + 6.2, r.top()), pos2(r.right() - 0.8, r.top())], s);
        }
        Ico::Drums => {
            p.rect_stroke(Rect::from_center_size(c, vec2(15.0, 9.0)), 4.0, s, egui::StrokeKind::Middle);
            p.line_segment([pos2(r.left(), c.y - 1.0), pos2(r.right(), c.y - 1.0)], s);
        }
        Ico::Instruments | Ico::Rack => {
            p.rect_stroke(r, 0.0, s, egui::StrokeKind::Middle);
            for i in 1..5 {
                let x = r.left() + i as f32 * 3.0;
                p.line_segment([pos2(x, r.top()), pos2(x, r.top() + if ic == Ico::Rack { 11.0 } else { 6.0 })], s);
            }
        }
        Ico::AudioFx | Ico::Samples => {
            for i in 0..6 {
                let x = r.left() + 1.5 + i as f32 * 2.5;
                let h = [3.0, 7.0, 10.0, 6.0, 9.0, 4.0][i];
                p.line_segment([pos2(x, c.y - h / 2.0), pos2(x, c.y + h / 2.0)], s);
            }
        }
        Ico::MidiFx | Ico::Templates | Ico::Clips => {
            for i in 0..3 {
                let y = r.top() + 2.0 + i as f32 * 3.5;
                p.line_segment([pos2(r.left() + 1.0 + (i % 2) as f32 * 4.0, y), pos2(r.right() - 1.0, y)], s);
            }
        }
        Ico::Modulators | Ico::Grooves => {
            let pts: Vec<Pos2> = (0..=14).map(|i| pos2(r.left() + i as f32, c.y - ((i as f32) * 0.9).sin() * 4.0)).collect();
            p.add(egui::Shape::line(pts, s));
        }
        Ico::Max | Ico::Plugins => {
            p.rect_stroke(r.shrink(1.0), 1.0, s, egui::StrokeKind::Middle);
            p.circle_stroke(c, 2.0, s);
        }
        Ico::Folder => {
            let pts = vec![
                pos2(r.left(), r.bottom()),
                pos2(r.left(), r.top()),
                pos2(r.left() + 5.0, r.top()),
                pos2(r.left() + 6.5, r.top() + 2.0),
                pos2(r.right(), r.top() + 2.0),
                pos2(r.right(), r.bottom()),
                pos2(r.left(), r.bottom()),
            ];
            p.add(egui::Shape::line(pts, s));
        }
        Ico::Preset => {
            p.rect_stroke(r, 0.0, s, egui::StrokeKind::Middle);
            p.line_segment([pos2(r.left() + 3.0, c.y), pos2(r.left() + 7.0, c.y)], s);
            p.line_segment([pos2(r.left() + 3.0, c.y + 2.5), pos2(r.right() - 3.0, c.y + 2.5)], s);
        }
    }
}

/// One list row. `open`: Some(state) for expandable rows.
fn row(ui: &mut egui::Ui, indent: usize, open: Option<bool>, ic: Ico, name: &str, selected: bool, sense: Sense) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), sense);
    let p = ui.painter();
    if selected {
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), ROW - 1.0)), 0.0, SEL_ROW);
    } else if resp.hovered() {
        p.rect_filled(r, 0.0, gray(0x3c));
    }
    let x = r.left() + 8.0 + indent as f32 * 12.0;
    let fgc = if selected { Color32::from_gray(0xf0) } else { text() };
    if let Some(o) = open {
        let c = pos2(x + 3.0, r.center().y);
        let pts = if o {
            vec![c + vec2(-3.5, -2.0), c + vec2(3.5, -2.0), c + vec2(0.0, 2.5)]
        } else {
            vec![c + vec2(-2.0, -3.5), c + vec2(2.5, 0.0), c + vec2(-2.0, 3.5)]
        };
        p.add(egui::Shape::convex_polygon(pts, gray(0x8a), Stroke::NONE));
    }
    icon(p, ic, pos2(x + 20.0, r.center().y), if selected { fgc } else { gray(0xa0) });
    p.text(pos2(x + 33.0, r.center().y), Align2::LEFT_CENTER, name, font(FS), fgc);
    resp
}

/// Draggable leaf item.
#[allow(clippy::too_many_arguments)]
fn leaf(
    app: &mut DawApp,
    ui: &mut egui::Ui,
    st: &mut State,
    indent: usize,
    ic: Ico,
    name: &str,
    key: &str,
    payload: DragItem,
    preview: Option<&str>,
) {
    let resp = row(ui, indent, None, ic, name, st.selected.as_deref() == Some(key), Sense::click_and_drag());
    resp.dnd_set_drag_payload(payload);
    if resp.dragged() {
        super::cursors::set(ui.ctx(), super::cursors::Cur::Drag);
    }
    if resp.clicked() || resp.drag_started() {
        st.selected = Some(key.to_string());
        if let (true, Some(k)) = (resp.clicked() && app.preview_on, preview) {
            app.preview_sample(k);
        }
    }
    if let Some(k) = preview
        && let Some(e) = app.cache.error(k)
    {
        resp.on_hover_text(format!("Failed: {e}"));
    }
}

/// Expandable folder row; returns whether it is open.
fn folder(ui: &mut egui::Ui, st: &mut State, indent: usize, ic: Ico, name: &str) -> bool {
    let o = st.open.iter().any(|n| n == name);
    let resp = row(ui, indent, Some(o), ic, name, false, Sense::click());
    if resp.clicked() {
        if o {
            st.open.retain(|n| n != name);
        } else {
            st.open.push(name.to_string());
        }
    }
    o != resp.clicked()
}

fn instruments(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize) {
    if folder(ui, st, ind, Ico::Rack, "Drum Rack") {
        leaf(app, ui, st, ind + 1, Ico::Preset, "Factory Kit", "kit", DragItem::DrumKit, None);
    }
    if folder(ui, st, ind, Ico::Instruments, "Synth") {
        sounds(app, ui, st, ind + 1);
    }
}

fn sounds(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize) {
    for p in daw_engine::SynthParams::PRESETS {
        leaf(app, ui, st, ind, Ico::Preset, p, &format!("syn:{p}"), DragItem::SynthPreset(p.to_string()), None);
    }
}

fn samples(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize) {
    for n in daw_core::factory::NAMES {
        let key = format!("{}{n}", daw_project::FACTORY_PREFIX);
        leaf(app, ui, st, ind, Ico::Samples, n, &key.clone(), DragItem::Sample { key: key.clone(), name: n.to_string() }, Some(&key));
    }
}

pub fn browser(app: &mut DawApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("browser_state");
    let mut st: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let full = ui.max_rect();
    ui.allocate_rect(full, Sense::hover());
    let p = ui.painter().clone();
    let hline =
        |y: f32, a: f32, b: f32| p.rect_filled(Rect::from_min_max(pos2(a, full.top() + y), pos2(b, full.top() + y + 2.0)), 0.0, sep());
    // Search row (greyed): ‹ › nav, search field, filter switch, ▾.
    let dimc = gray(0x62);
    let top = Rect::from_min_size(full.min, vec2(full.width(), 21.0));
    for (i, ch) in ["‹", "›"].iter().enumerate() {
        p.text(pos2(top.left() + 9.0 + i as f32 * 17.0, top.center().y), Align2::CENTER_CENTER, *ch, font(12.0), dimc);
    }
    p.line_segment([pos2(top.left() + 17.5, top.top() + 4.0), pos2(top.left() + 17.5, top.bottom() - 4.0)], Stroke::new(1.0, sep()));
    let search = Rect::from_min_max(pos2(top.left() + 36.0, top.top() + 1.0), pos2(top.right() - 41.0, top.bottom()));
    p.rect_filled(search, 0.0, gray(0x2e));
    p.text(search.left_center() + vec2(9.0, 0.0), Align2::LEFT_CENTER, "Search (Ctrl + F)", font(FS), gray(0x5a));
    ui.interact(search, ui.make_persistent_id("brw_search"), Sense::hover()).on_hover_text("Search (not available in this version)");
    let fb = Rect::from_min_size(pos2(top.right() - 38.0, top.top() + 3.0), vec2(16.0, 15.0));
    p.rect_filled(fb, 0.0, gray(0x46));
    for (i, w) in [9.0, 6.0, 3.0].iter().enumerate() {
        let y = fb.top() + 4.0 + i as f32 * 3.0;
        p.line_segment([pos2(fb.center().x - w / 2.0, y), pos2(fb.center().x + w / 2.0, y)], Stroke::new(1.0, dimc));
    }
    let a = pos2(top.right() - 11.0, top.center().y + 1.5);
    p.add(egui::Shape::convex_polygon(vec![a + vec2(-3.5, -3.5), a + vec2(3.5, -3.5), a], dimc, Stroke::NONE));
    hline(21.0, full.left(), full.right());
    // Sidebar.
    let side = Rect::from_min_max(pos2(full.left(), full.top() + 23.0), pos2(full.left() + SIDE_W, full.bottom()));
    p.rect_filled(Rect::from_min_max(pos2(side.right(), side.top()), pos2(side.right() + 2.0, full.bottom())), 0.0, sep());
    p.text(pos2(side.center().x, side.top() + 9.0), Align2::CENTER_CENTER, "Co.", font(FS_SMALL), dim());
    for (i, c) in COLLECTIONS.iter().enumerate() {
        let r = Rect::from_min_size(pos2(side.left() + 10.0, side.top() + 24.0 + i as f32 * 19.0), vec2(7.0, 7.0));
        p.rect_filled(r, 0.0, rgb(*c).gamma_multiply(0.55));
        ui.interact(r.expand(5.0), ui.make_persistent_id(("brw_col", i)), Sense::hover())
            .on_hover_text("Collections (not available in this version)");
    }
    let lib_y = side.top() + 178.0;
    p.text(pos2(side.center().x, lib_y), Align2::CENTER_CENTER, "Lib.", font(FS_SMALL), dim());
    let mut entries: Vec<(Ico, &str, Cat)> = LIB.to_vec();
    entries.push((Ico::Folder, "Places: Files", Cat::Files));
    for (i, (ic, name, cat)) in entries.iter().enumerate() {
        let r = Rect::from_min_size(pos2(side.left(), lib_y + 12.0 + i as f32 * 19.0), vec2(SIDE_W, 18.0));
        if r.bottom() > side.bottom() {
            break;
        }
        let enabled = *cat != Cat::Other;
        let resp = ui.interact(r, ui.make_persistent_id(("brw_lib", i)), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && st.cat == *cat {
            p.rect_filled(r, 0.0, SEL_ROW);
        } else if enabled && resp.hovered() {
            p.rect_filled(r, 0.0, gray(0x40));
        }
        icon(
            &p,
            *ic,
            r.center(),
            if !enabled {
                gray(0x5a)
            } else if st.cat == *cat {
                Color32::from_gray(0xf0)
            } else {
                text()
            },
        );
        if resp.clicked() {
            st.cat = *cat;
        }
        resp.on_hover_text(if enabled { name.to_string() } else { format!("{name} (not available in this version)") });
    }
    // Content filter (greyed).
    let lx = side.right() + 2.0;
    let filt = Rect::from_min_max(pos2(lx, full.top() + 23.0), pos2(full.right(), full.top() + 76.0));
    p.text(filt.left_top() + vec2(5.0, 10.0), Align2::LEFT_CENTER, "Content ▾", font(FS), dimc);
    let mut x = filt.left() + 4.0;
    let mut y = filt.top() + 20.0;
    for chip in ["Backup Set", "Device", "Groove", "MIDI Clip", "MIDI Tool", "Preset", "Sample", "Set", "Tuning"] {
        let g = p.layout_no_wrap(chip.to_string(), font(FS), dimc);
        let w = g.size().x + 8.0;
        if x + w > filt.right() - 4.0 {
            x = filt.left() + 4.0;
            y += 16.0;
        }
        let r = Rect::from_min_size(pos2(x, y), vec2(w, 13.0));
        p.rect_filled(r, 0.0, gray(0x40));
        p.galley(pos2(r.left() + 4.0, r.center().y - g.size().y / 2.0), g, dimc);
        x += w + 3.0;
    }
    ui.interact(filt, ui.make_persistent_id("brw_filter"), Sense::hover()).on_hover_text("Filters (not available in this version)");
    hline(76.0, lx, full.right());
    // Name header.
    let hdr = Rect::from_min_max(pos2(lx, full.top() + 78.0), pos2(full.right(), full.top() + 97.0));
    p.rect_filled(hdr, 0.0, gray(0x46));
    p.text(hdr.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, "Name", bold(FS), text());
    let a = pos2(hdr.right() - 24.0, hdr.center().y);
    p.add(egui::Shape::convex_polygon(vec![a + vec2(0.0, -3.5), a + vec2(3.5, 3.0), a + vec2(-3.5, 3.0)], gray(0x6a), Stroke::NONE));
    p.circle_filled(pos2(hdr.right() - 9.0, hdr.center().y), 5.5, gray(0x22));
    for i in -1..=1 {
        p.circle_filled(pos2(hdr.right() - 9.0 + i as f32 * 2.5, hdr.center().y), 0.8, text());
    }
    // Footer: Tags (greyed) and Preview.
    let foot_top = full.bottom() - 47.0;
    hline(foot_top - full.top(), lx, full.right());
    let tags = Rect::from_min_max(pos2(lx, foot_top + 2.0), pos2(full.right(), foot_top + 25.0));
    p.text(tags.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, "Tags:", font(FS), dimc);
    p.text(tags.left_center() + vec2(42.0, 0.0), Align2::LEFT_CENTER, "Add…", font(FS), gray(0x55));
    ui.interact(tags, ui.make_persistent_id("brw_tags"), Sense::hover()).on_hover_text("Tags (not available in this version)");
    p.rect_filled(Rect::from_min_max(pos2(lx, foot_top + 25.0), pos2(full.right(), foot_top + 26.0)), 0.0, sep());
    let prev = Rect::from_min_max(pos2(lx, foot_top + 26.0), pos2(full.right(), full.bottom()));
    let hp = Rect::from_center_size(pos2(prev.left() + 12.0, prev.center().y), vec2(14.0, 14.0));
    let resp = ui
        .interact(hp, ui.make_persistent_id("brw_preview"), Sense::click())
        .on_hover_text("Preview: click a sample to hear it on the Cue bus");
    let on = app.preview_on;
    p.circle_filled(hp.center(), 6.5, if on { Color32::from_rgb(0x1e, 0x5f, 0xb4) } else { gray(0x24) });
    let hc = if on { Color32::from_rgb(0x0e, 0x1e, 0x33) } else { gray(0x80) };
    p.circle_stroke(hp.center() + vec2(0.0, 0.5), 3.5, Stroke::new(1.3, hc));
    p.rect_filled(Rect::from_center_size(hp.center() + vec2(-3.5, 2.0), vec2(2.0, 3.0)), 0.0, hc);
    p.rect_filled(Rect::from_center_size(hp.center() + vec2(3.5, 2.0), vec2(2.0, 3.0)), 0.0, hc);
    if resp.clicked() {
        app.preview_on = !on;
        if on {
            app.stop_preview();
        }
    }
    let status = if st.selected.is_some() { "1 item selected" } else { "" };
    p.text(prev.center(), Align2::CENTER_CENTER, status, font(FS), text());
    // List.
    let list = Rect::from_min_max(pos2(lx, hdr.bottom() + 1.0), pos2(full.right(), foot_top));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list).layout(egui::Layout::top_down(egui::Align::Min)));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    egui::ScrollArea::vertical().id_salt(("browser", st.cat as u8)).auto_shrink([false, false]).show(&mut child, |ui| match st.cat {
        Cat::All => {
            if folder(ui, &mut st, 0, Ico::Sounds, "Sounds") {
                sounds(app, ui, &mut st, 1);
            }
            if folder(ui, &mut st, 0, Ico::Drums, "Drums") {
                leaf(app, ui, &mut st, 1, Ico::Rack, "Factory Kit", "kit", DragItem::DrumKit, None);
            }
            if folder(ui, &mut st, 0, Ico::Instruments, "Instruments") {
                instruments(app, ui, &mut st, 1);
            }
            if folder(ui, &mut st, 0, Ico::Samples, "Samples") {
                samples(app, ui, &mut st, 1);
            }
            if folder(ui, &mut st, 0, Ico::Folder, "Files") {
                files(app, ui, &mut st, 1);
            }
        }
        Cat::Sounds => sounds(app, ui, &mut st, 0),
        Cat::Drums => {
            leaf(app, ui, &mut st, 0, Ico::Rack, "Factory Kit", "kit", DragItem::DrumKit, None);
            samples(app, ui, &mut st, 0);
        }
        Cat::Instruments => instruments(app, ui, &mut st, 0),
        Cat::Samples => samples(app, ui, &mut st, 0),
        Cat::Files => files(app, ui, &mut st, 0),
        Cat::Other => {}
    });
    ui.data_mut(|d| d.insert_temp(id, st));
}

fn files(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize) {
    // Current folder (click ".." to go up, a folder to enter it).
    let dir_name = app.browser_dir.to_string_lossy().into_owned();
    let r = row(ui, ind, Some(true), Ico::Folder, &dir_name, false, Sense::hover());
    r.on_hover_text(&dir_name);
    if let Some(parent) = app.browser_dir.parent()
        && row(ui, ind + 1, None, Ico::Folder, "..", false, Sense::click()).clicked()
    {
        app.browser_dir = parent.to_path_buf();
    }
    // Directory listing is re-read only when the folder changes (cached in egui memory).
    let id = egui::Id::unique("browser_listing");
    let cached: Option<(PathBuf, Vec<(bool, PathBuf)>)> = ui.data(|d| d.get_temp(id));
    let entries = match cached {
        Some((d, e)) if d == app.browser_dir => e,
        _ => {
            let mut e: Vec<(bool, PathBuf)> = std::fs::read_dir(&app.browser_dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.path())
                        .filter(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')))
                        .filter_map(|p| {
                            if p.is_dir() {
                                Some((true, p))
                            } else if p
                                .extension()
                                .and_then(|e| e.to_str())
                                .is_some_and(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
                            {
                                Some((false, p))
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            e.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            e.truncate(2000);
            ui.data_mut(|d| d.insert_temp(id, (app.browser_dir.clone(), e.clone())));
            e
        }
    };
    for (is_dir, path) in entries {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if is_dir {
            if row(ui, ind + 1, Some(false), Ico::Folder, &name, false, Sense::click()).clicked() {
                app.browser_dir = path;
            }
        } else {
            let key = path.to_string_lossy().into_owned();
            let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name.clone());
            leaf(app, ui, st, ind + 1, Ico::Samples, &name, &key.clone(), DragItem::Sample { key: key.clone(), name: stem }, Some(&key));
        }
    }
}
