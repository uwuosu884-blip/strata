//! Live 12 browser, measured from hh-browser-*.png: search row with working
//! back / forward history and search, resizable sidebar (narrow "Co. / Lib. /
//! Pl." icons or wide labelled), colour collections (edit / hide, item dots,
//! keys 1-7), "Name" list with optional extra columns (3-dots menu), hidden
//! filter view (filter button) and a preview footer. Our content: instruments,
//! factory sounds and the file system; Live categories we don't have are greyed.

use super::menu;
use super::theme::*;
use super::widgets::rgb;
use crate::app::{DawApp, DragItem};
use eframe::egui::{self, Align2, Color32, Key, Pos2, Rect, Sense, Stroke, pos2, vec2};
use std::path::{Path, PathBuf};

const AUDIO_EXT: [&str; 10] = ["wav", "flac", "mp3", "ogg", "opus", "aif", "aiff", "m4a", "aac", "wave"];
const ROW: f32 = 19.0;
const NARROW_W: f32 = 27.0;
/// Panel-relative y of the "Name" header without the filter view.
const HDR_Y: f32 = 23.0;
const FOOT_H: f32 = 21.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    All,
    Sounds,
    Drums,
    Instruments,
    Samples,
    Files,
    Collection(u8),
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
    Tunings,
    Templates,
    Folder,
    Preset,
    Rack,
    Packs,
    User,
    Project,
    AddFolder,
}

/// Library entries: (icon, name, category; Other = greyed).
const LIB: [(Ico, &str, Cat); 14] = [
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
    (Ico::Tunings, "Tunings", Cat::Other),
    (Ico::Templates, "Templates", Cat::Other),
];

pub const COLLECTIONS: [([u8; 3], &str); 7] = [
    ([0xff, 0x05, 0x05], "Favorites"),
    ([0xff, 0x9e, 0x17], "Orange"),
    ([0xff, 0xee, 0x34], "Yellow"),
    ([0x1a, 0xff, 0x9f], "Green"),
    ([0x00, 0x95, 0xff], "Blue"),
    ([0xa6, 0x66, 0xff], "Purple"),
    ([0x77, 0x77, 0x77], "Gray"),
];

/// Extra list columns (3-dots menu): name, width.
const COLS: [(&str, f32); 5] = [("Date Modified", 104.0), ("Size", 60.0), ("Type", 56.0), ("Rank", 40.0), ("Place", 56.0)];

/// Navigation point for back / forward.
#[derive(Clone, PartialEq)]
struct Nav {
    cat: Cat,
    dir: PathBuf,
}

/// Browser UI state (egui memory; collections / columns live in Settings).
#[derive(Clone)]
struct State {
    cat: Cat,
    open: Vec<String>,
    selected: Option<String>,
    back: Vec<Nav>,
    fwd: Vec<Nav>,
    search: String,
    filters: bool,
    coll_edit: bool,
    focused: bool,
    // Per-frame layout (set before the list is drawn).
    cols: u8,
    ext: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            cat: Cat::All,
            open: vec!["Instruments".into(), "Drum Rack".into(), "Synth".into()],
            selected: None,
            back: Vec::new(),
            fwd: Vec::new(),
            search: String::new(),
            filters: false,
            coll_edit: false,
            focused: false,
            cols: 0,
            ext: true,
        }
    }
}

/// Per-item data for dots and extra columns.
#[derive(Clone, Copy, Default)]
struct Info {
    mask: u8,
    size: Option<u64>,
    mtime: Option<u64>,
    kind: &'static str,
    place: &'static str,
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
        Ico::Tunings => {
            for i in 0..5 {
                let x = r.left() + 1.5 + i as f32 * 3.0;
                p.line_segment([pos2(x, r.top() + i as f32), pos2(x, r.bottom())], s);
            }
        }
        Ico::Packs => {
            p.rect_stroke(
                Rect::from_min_max(pos2(r.left(), r.top() + 2.0), pos2(r.right() - 2.0, r.bottom())),
                0.0,
                s,
                egui::StrokeKind::Middle,
            );
            p.line_segment([pos2(r.left() + 2.0, r.top()), pos2(r.right(), r.top())], s);
            p.line_segment([pos2(r.right(), r.top()), pos2(r.right(), r.bottom() - 2.0)], s);
            p.line_segment([pos2(r.left() + 2.0, c.y - 1.0), pos2(r.right() - 4.0, c.y - 1.0)], s);
            p.line_segment([pos2(r.left() + 2.0, c.y + 3.0), pos2(r.right() - 4.0, c.y + 3.0)], s);
        }
        Ico::User => {
            p.circle_stroke(pos2(c.x, r.top() + 2.0), 3.0, s);
            p.rect_stroke(
                Rect::from_min_max(pos2(c.x - 7.0, r.bottom() - 4.0), pos2(c.x + 7.0, r.bottom() + 1.0)),
                1.5,
                s,
                egui::StrokeKind::Middle,
            );
        }
        Ico::Project => {
            p.rect_stroke(r, 0.0, s, egui::StrokeKind::Middle);
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-2.0, -3.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 3.0)], col, Stroke::NONE));
        }
        Ico::AddFolder => {
            p.rect_stroke(r, 0.0, s, egui::StrokeKind::Middle);
            p.line_segment([pos2(c.x - 3.0, c.y), pos2(c.x + 3.0, c.y)], s);
            p.line_segment([pos2(c.x, c.y - 3.0), pos2(c.x, c.y + 3.0)], s);
        }
        Ico::Preset => {
            p.rect_stroke(r, 0.0, s, egui::StrokeKind::Middle);
            p.line_segment([pos2(r.left() + 3.0, c.y), pos2(r.left() + 7.0, c.y)], s);
            p.line_segment([pos2(r.left() + 3.0, c.y + 2.5), pos2(r.right() - 3.0, c.y + 2.5)], s);
        }
    }
}

/// Middle-elided text ("Crash Cy...RY).wav") fitting `max_w`.
fn elide(p: &egui::Painter, s: &str, max_w: f32) -> String {
    let w = |t: &str| p.layout_no_wrap(t.to_string(), font(FS), Color32::WHITE).size().x;
    if max_w <= 0.0 || w(s) <= max_w {
        return s.to_string();
    }
    let ch: Vec<char> = s.chars().collect();
    let mut keep = ch.len().saturating_sub(1);
    while keep > 1 {
        let a = keep.div_ceil(2);
        let b = keep / 2;
        let t: String = ch[..a].iter().chain("...".chars().collect::<Vec<_>>().iter()).chain(ch[ch.len() - b..].iter()).collect();
        if w(&t) <= max_w {
            return t;
        }
        keep -= 1;
    }
    "...".into()
}

/// "2026-10-29 14:03" (UTC) from Unix seconds.
fn fmt_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (h, mi) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    // Civil date from days since 1970-01-01 (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y}-{m:02}-{d:02} {h:02}:{mi:02}")
}

fn fmt_size(b: u64) -> String {
    if b < 1024 * 1024 { format!("{} KB", b.div_ceil(1024)) } else { format!("{:.1} MB", b as f64 / 1048576.0) }
}

/// 5 × 5 collection dots, 7 px pitch, ending at `right`.
fn dots(p: &egui::Painter, mask: u8, right: f32, top: f32) {
    let n = mask.count_ones() as f32;
    let mut x = right - n * 7.0 + 2.0;
    for (i, (c, _)) in COLLECTIONS.iter().enumerate() {
        if mask & (1 << i) != 0 {
            p.rect_filled(Rect::from_min_size(pos2(x, top), vec2(5.0, 5.0)), 1.0, rgb(*c));
            x += 7.0;
        }
    }
}

/// Selection colours: Live's blue when the browser has focus, else dimmed.
fn sel_colors(focused: bool) -> (Color32, Color32) {
    if focused { (SEL_BLUE, gray(0x12)) } else { (SEL_ROW, gray(0xf0)) }
}

/// One list row. `open`: Some(state) for expandable rows; `info` for items.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    st: &State,
    indent: usize,
    open: Option<bool>,
    ic: Ico,
    name: &str,
    selected: bool,
    sense: Sense,
    info: Option<Info>,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), sense);
    if !ui.is_rect_visible(r) {
        return resp;
    }
    let p = ui.painter();
    let (sel_bg, sel_fg) = sel_colors(st.focused);
    if selected {
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), ROW - 1.0)), 0.0, sel_bg);
    } else if resp.hovered() {
        p.rect_filled(r, 0.0, gray(0x3c));
    }
    let x = r.left() + 8.0 + indent as f32 * 12.0;
    let fgc = if selected { sel_fg } else { text() };
    if let Some(o) = open {
        let c = pos2(x + 1.5, r.center().y);
        let pts = if o {
            vec![c + vec2(-3.5, -2.0), c + vec2(3.5, -2.0), c + vec2(0.0, 2.5)]
        } else {
            vec![c + vec2(-2.0, -3.5), c + vec2(2.5, 0.0), c + vec2(-2.0, 3.5)]
        };
        p.add(egui::Shape::convex_polygon(pts, if selected { sel_fg } else { gray(0x8a) }, Stroke::NONE));
    }
    icon(p, ic, pos2(x + 18.0, r.center().y), if selected { fgc } else { gray(0xa0) });
    // Extra columns from the right edge; dots end 15 px from it (5 px before the columns).
    let mut name_r = r.right();
    let mut cols: Vec<(usize, f32, f32)> = Vec::new();
    for (i, (_, w)) in COLS.iter().enumerate().rev() {
        if st.cols & (1 << i) != 0 {
            cols.push((i, name_r - w, name_r));
            name_r -= w;
        }
    }
    let dots_r = if cols.is_empty() { r.right() - 15.0 } else { name_r - 5.0 };
    let info = info.unwrap_or_default();
    let mut text_r = name_r - 4.0;
    if info.mask != 0 {
        dots(p, info.mask, dots_r, r.top() + 6.0);
        text_r = dots_r - info.mask.count_ones() as f32 * 7.0 - 2.0;
    }
    let shown = if info.size.is_some() && !st.ext {
        Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name.into())
    } else {
        name.to_string()
    };
    p.text(pos2(x + 30.0, r.center().y), Align2::LEFT_CENTER, elide(p, &shown, text_r - x - 30.0), font(FS), fgc);
    for (i, a, b) in cols {
        let v = match i {
            0 => info.mtime.map(fmt_date).unwrap_or_default(),
            1 => info.size.map(fmt_size).unwrap_or_default(),
            2 => info.kind.to_string(),
            4 => info.place.to_string(),
            _ => String::new(),
        };
        if i == 1 {
            p.text(pos2(b - 13.0, r.center().y), Align2::RIGHT_CENTER, v, font(FS), fgc);
        } else {
            p.text(pos2(a + 5.0, r.center().y), Align2::LEFT_CENTER, elide(p, &v, b - a - 8.0), font(FS), fgc);
        }
    }
    resp
}

/// Draggable leaf item (collections via keys 1-7 or its context menu).
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
    mut info: Info,
) {
    info.mask = app.settings.browser.collections.get(key).copied().unwrap_or(0);
    let resp = row(ui, st, indent, None, ic, name, st.selected.as_deref() == Some(key), Sense::click_and_drag(), Some(info));
    resp.dnd_set_drag_payload(payload);
    if resp.dragged() {
        super::cursors::set(ui.ctx(), super::cursors::Cur::Drag);
    }
    if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
        st.selected = Some(key.to_string());
        if let (true, Some(k)) = (resp.clicked() && app.preview_on, preview) {
            app.preview_sample(k);
        }
    }
    menu::context(&resp, 180.0, |ui| {
        for (i, (_, n)) in COLLECTIONS.iter().enumerate() {
            if menu::check_item(ui, n, &(i + 1).to_string(), info.mask & (1 << i) != 0, true) {
                toggle_collection(app, key, i);
            }
        }
    });
    if let Some(k) = preview
        && let Some(e) = app.cache.error(k)
    {
        resp.on_hover_text(format!("Failed: {e}"));
    }
}

fn toggle_collection(app: &mut DawApp, key: &str, i: usize) {
    let m = app.settings.browser.collections.entry(key.to_string()).or_insert(0);
    *m ^= 1 << i;
    if *m == 0 {
        app.settings.browser.collections.remove(key);
    }
    app.save_settings();
}

/// Expandable folder row; returns whether it is open.
fn folder(ui: &mut egui::Ui, st: &mut State, indent: usize, ic: Ico, name: &str) -> bool {
    let o = st.open.iter().any(|n| n == name);
    let resp = row(ui, st, indent, Some(o), ic, name, false, Sense::click(), None);
    if resp.clicked() {
        if o {
            st.open.retain(|n| n != name);
        } else {
            st.open.push(name.to_string());
        }
    }
    o != resp.clicked()
}

const LIB_INFO: fn(&'static str) -> Info = |kind| Info { kind, place: "Library", ..Default::default() };

fn instruments(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize) {
    if folder(ui, st, ind, Ico::Rack, "Drum Rack") {
        leaf(app, ui, st, ind + 1, Ico::Preset, "Factory Kit", "kit", DragItem::DrumKit, None, LIB_INFO("Rack"));
    }
    if folder(ui, st, ind, Ico::Instruments, "Synth") {
        sounds(app, ui, st, ind + 1, "");
    }
}

fn sounds(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize, q: &str) {
    for p in daw_engine::SynthParams::PRESETS {
        if matches(p, q) {
            leaf(app, ui, st, ind, Ico::Preset, p, &format!("syn:{p}"), DragItem::SynthPreset(p.to_string()), None, LIB_INFO("Preset"));
        }
    }
}

fn samples(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize, q: &str) {
    for n in daw_core::factory::NAMES {
        if matches(n, q) {
            let key = format!("{}{n}", daw_project::FACTORY_PREFIX);
            leaf(
                app,
                ui,
                st,
                ind,
                Ico::Samples,
                n,
                &key,
                DragItem::Sample { key: key.clone(), name: n.to_string() },
                Some(&key),
                LIB_INFO("Sample"),
            );
        }
    }
}

fn matches(name: &str, q: &str) -> bool {
    q.is_empty() || name.to_lowercase().contains(q)
}

/// A collection item by key (presets, kit, factory samples, files).
fn keyed_item(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, key: &str) {
    if key == "kit" {
        leaf(app, ui, st, 0, Ico::Rack, "Factory Kit", "kit", DragItem::DrumKit, None, LIB_INFO("Rack"));
    } else if let Some(p) = key.strip_prefix("syn:") {
        leaf(app, ui, st, 0, Ico::Preset, p, key, DragItem::SynthPreset(p.to_string()), None, LIB_INFO("Preset"));
    } else if let Some(n) = key.strip_prefix(daw_project::FACTORY_PREFIX) {
        leaf(
            app,
            ui,
            st,
            0,
            Ico::Samples,
            n,
            key,
            DragItem::Sample { key: key.to_string(), name: n.to_string() },
            Some(key),
            LIB_INFO("Sample"),
        );
    } else {
        let path = PathBuf::from(key);
        let meta = std::fs::metadata(&path).ok();
        file_leaf(app, ui, st, 0, &path, meta.as_ref().map(|m| m.len()).unwrap_or(0), meta.and_then(|m| mtime(&m)));
    }
}

fn mtime(m: &std::fs::Metadata) -> Option<u64> {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs())
}

fn file_leaf(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize, path: &Path, size: u64, mt: Option<u64>) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let key = path.to_string_lossy().into_owned();
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name.clone());
    let info = Info { size: Some(size), mtime: mt, kind: "Sample", place: "Files", ..Default::default() };
    leaf(app, ui, st, ind, Ico::Samples, &name, &key, DragItem::Sample { key: key.clone(), name: stem }, Some(&key), info);
}

/// Folder listing (dirs first, audio files), cached until the folder changes.
type Listing = Vec<(bool, PathBuf, u64, Option<u64>)>;
fn listing(ui: &egui::Ui, dir: &Path) -> Listing {
    let id = egui::Id::unique("browser_listing");
    if let Some((d, e)) = ui.data(|d| d.get_temp::<(PathBuf, Listing)>(id))
        && d == dir
    {
        return e;
    }
    let mut e: Listing = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .filter_map(|e| {
                    let p = e.path();
                    let m = std::fs::metadata(&p).ok()?;
                    if m.is_dir() {
                        Some((true, p, 0, mtime(&m)))
                    } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| AUDIO_EXT.contains(&x.to_ascii_lowercase().as_str())) {
                        Some((false, p, m.len(), mtime(&m)))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    e.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    e.truncate(2000);
    ui.data_mut(|d| d.insert_temp(id, (dir.to_path_buf(), e.clone())));
    e
}

fn files(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, ind: usize, q: &str) {
    if q.is_empty() {
        // Current folder (".." goes up, a folder enters it; both are history steps).
        let dir_name = app.browser_dir.to_string_lossy().into_owned();
        row(ui, st, ind, Some(true), Ico::Folder, &dir_name, false, Sense::hover(), None).on_hover_text(&dir_name);
        if let Some(parent) = app.browser_dir.parent().map(Path::to_path_buf)
            && row(ui, st, ind + 1, None, Ico::Folder, "..", false, Sense::click(), None).clicked()
        {
            go(app, st, st.cat, parent);
        }
    }
    let dir = app.browser_dir.clone();
    for (is_dir, path, size, mt) in listing(ui, &dir) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !matches(&name, q) {
            continue;
        }
        if is_dir {
            let info = Info { mtime: mt, kind: "Folder", place: "Files", ..Default::default() };
            if row(ui, st, ind + 1, Some(false), Ico::Folder, &name, false, Sense::click(), Some(info)).clicked() {
                go(app, st, st.cat, path);
            }
        } else {
            file_leaf(app, ui, st, ind + 1, &path, size, mt);
        }
    }
}

/// Navigate (recorded for back / forward).
fn go(app: &mut DawApp, st: &mut State, cat: Cat, dir: PathBuf) {
    let cur = Nav { cat: st.cat, dir: app.browser_dir.clone() };
    let next = Nav { cat, dir };
    if cur != next {
        st.back.push(cur);
        st.back.truncate(64);
        st.fwd.clear();
        st.cat = next.cat;
        app.browser_dir = next.dir;
    }
}

fn step(app: &mut DawApp, st: &mut State, back: bool) {
    let (from, to) = if back { (&mut st.back, &mut st.fwd) } else { (&mut st.fwd, &mut st.back) };
    if let Some(n) = from.pop() {
        to.push(Nav { cat: st.cat, dir: app.browser_dir.clone() });
        st.cat = n.cat;
        app.browser_dir = n.dir;
    }
}

/// Small chevron (‹ or ›).
fn chevron(p: &egui::Painter, c: Pos2, left: bool, col: Color32) {
    let d = if left { 1.5 } else { -1.5 };
    p.add(egui::Shape::line(vec![c + vec2(d, -3.0), c + vec2(-d, 0.0), c + vec2(d, 3.0)], Stroke::new(1.2, col)));
}

pub fn browser(app: &mut DawApp, ui: &mut egui::Ui) {
    let id = egui::Id::unique("browser_state");
    let mut st: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let full = ui.max_rect();
    ui.allocate_rect(full, Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(full, 3.0, panel());
    st.focused = ui.data(|d| d.get_temp::<bool>(egui::Id::unique("focus_browser"))).unwrap_or(false);
    st.cols = app.settings.browser.columns;
    st.ext = app.settings.browser.show_extensions;
    let hfill = |r: Rect| p.rect_filled(r, 0.0, sep());
    let at = |x: f32, y: f32| pos2(full.left() + x, full.top() + y);

    // Search row: ‹ | › || search || filter ▾.
    let enabled_c = text();
    let off_c = gray(0x5c);
    let rowc = |x0: f32, x1: f32| Rect::from_min_max(at(x0, 0.0), at(x1, 21.0));
    let back_r = rowc(0.0, 19.0);
    let fwd_r = rowc(20.0, 39.0);
    hfill(rowc(19.0, 20.0));
    hfill(rowc(39.0, 41.0));
    let w = full.width();
    let search_r = rowc(41.0, w - 41.0);
    hfill(rowc(w - 41.0, w - 39.0));
    let filt_r = rowc(w - 39.0, w - 18.0);
    let drop_r = rowc(w - 18.0, w);
    for (r, back) in [(back_r, true), (fwd_r, false)] {
        let can = if back { !st.back.is_empty() } else { !st.fwd.is_empty() };
        let resp = ui.interact(r, ui.make_persistent_id(("brw_nav", back)), if can { Sense::click() } else { Sense::hover() });
        chevron(&p, r.center(), back, if can { enabled_c } else { off_c });
        if resp.clicked() {
            step(app, &mut st, back);
        }
        resp.on_hover_text(if back { "Back" } else { "Forward" });
    }
    let search_id = egui::Id::unique("brw_search");
    let te = egui::TextEdit::singleline(&mut st.search)
        .id(search_id)
        .frame(egui::Frame::NONE)
        .font(font(FS))
        .text_color(text())
        .hint_text(egui::RichText::new("Search (Ctrl + F)").color(gray(0x6a)).font(font(FS)))
        .desired_width(search_r.width() - 12.0);
    let sresp = ui.put(Rect::from_min_max(search_r.min + vec2(7.0, 4.0), search_r.max - vec2(5.0, 3.0)), te);
    // Ctrl+Alt+F (Create Fades) is consumed earlier by the shortcuts.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::F)) {
        sresp.request_focus();
    }
    if sresp.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
        st.search.clear();
        sresp.surrender_focus();
    }
    // Filter-view switch: dark 17 × 13 box with a funnel of lines; orange when on.
    let fb = Rect::from_min_size(pos2(filt_r.left() + 4.0, filt_r.top() + 4.0), vec2(17.0, 13.0));
    let fresp = ui.interact(filt_r, ui.make_persistent_id("brw_filt"), Sense::click()).on_hover_text("Show / hide the filter view");
    p.rect_filled(fb, 1.0, gray(0x11));
    p.rect_filled(fb.shrink(1.0), 0.0, if st.filters { rgb([0xff, 0xad, 0x56]) } else { gray(0x1e) });
    let lc = if st.filters { gray(0x12) } else { text() };
    for (i, hw) in [3.5f32, 2.5, 1.5].iter().enumerate() {
        let y = fb.center().y - 2.5 + i as f32 * 2.5;
        p.line_segment([pos2(fb.center().x - hw, y), pos2(fb.center().x + hw, y)], Stroke::new(1.0, lc));
    }
    if fresp.clicked() {
        st.filters = !st.filters;
    }
    let dresp = ui.interact(drop_r, ui.make_persistent_id("brw_drop"), Sense::click());
    let a = drop_r.center() + vec2(-1.0, 1.5);
    p.add(egui::Shape::convex_polygon(vec![a + vec2(-3.0, -2.5), a + vec2(3.0, -2.5), a + vec2(0.0, 1.5)], text(), Stroke::NONE));
    let mut filters = st.filters;
    menu::dropdown(&dresp, 160.0, |ui| {
        if menu::check_item(ui, "Show Filter View", "", filters, true) {
            filters = !filters;
        }
    });
    st.filters = filters;
    hfill(Rect::from_min_max(at(0.0, 21.0), at(w, 23.0)));

    // Sidebar (resizable by dragging its right separator; snaps to the narrow icon column).
    let side_w = app.settings.browser.sidebar_w.clamp(NARROW_W, (w - 120.0).max(NARROW_W));
    let wide = side_w > 60.0;
    let side = Rect::from_min_max(at(0.0, 23.0), pos2(full.left() + side_w, full.bottom()));
    let sep_r = Rect::from_min_max(pos2(side.right(), side.top()), pos2(side.right() + 2.0, full.bottom()));
    hfill(sep_r);
    let sresp2 = ui.interact(sep_r.expand2(vec2(2.0, 0.0)), ui.make_persistent_id("brw_side_sep"), Sense::drag());
    if sresp2.hovered() || sresp2.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if sresp2.dragged()
        && let Some(px) = ui.ctx().pointer_latest_pos()
    {
        let nw = px.x - full.left();
        app.settings.browser.sidebar_w = if nw < 60.0 { NARROW_W } else { nw.clamp(90.0, (w - 120.0).max(90.0)) };
    }
    if sresp2.drag_stopped() {
        app.save_settings();
    }
    let sp = p.with_clip_rect(side);
    let (sel_bg, sel_fg) = sel_colors(st.focused);
    let label = |y: f32, short: &str, long: &str| {
        sp.text(at(6.0, y), Align2::LEFT_CENTER, if wide { long } else { short }, font(FS), dim());
    };
    // Collections: header (hover shows Edit; Done while editing), visible colours.
    label(31.0, "Co.", "Collections");
    let hdr_r = Rect::from_min_max(at(0.0, 23.0), pos2(side.right(), full.top() + 42.0));
    if wide {
        let hr = ui.interact(hdr_r, ui.make_persistent_id("brw_coll_hdr"), Sense::click());
        if st.coll_edit || hr.hovered() {
            sp.text(
                pos2(side.right() - 15.0, full.top() + 31.0),
                Align2::RIGHT_CENTER,
                if st.coll_edit { "Done" } else { "Edit" },
                font(FS),
                dim(),
            );
        }
        if hr.clicked() {
            st.coll_edit = !st.coll_edit;
        }
    } else {
        st.coll_edit = false;
    }
    let hidden = app.settings.browser.hidden;
    let mut y = 44.0;
    for (i, (c, name)) in COLLECTIONS.iter().enumerate() {
        let shown = hidden & (1 << i) == 0;
        if !shown && !st.coll_edit {
            continue;
        }
        let r = Rect::from_min_size(at(0.0, y), vec2(side_w, 18.0));
        y += 19.0;
        let resp = ui.interact(r, ui.make_persistent_id(("brw_coll", i)), Sense::click());
        let selected = !st.coll_edit && st.cat == Cat::Collection(i as u8);
        let hl = selected || (st.coll_edit && resp.hovered());
        if hl {
            sp.rect_filled(r, 0.0, if st.coll_edit { SEL_BLUE } else { sel_bg });
        } else if resp.hovered() {
            sp.rect_filled(r, 0.0, gray(0x40));
        }
        sp.rect_filled(Rect::from_min_size(pos2(r.left() + 9.0, r.top() + 3.0), vec2(7.0, 7.0)), 0.0, rgb(*c));
        let fg = if hl { if st.coll_edit { gray(0x12) } else { sel_fg } } else { text() };
        if wide {
            sp.text(pos2(r.left() + 25.0, r.center().y), Align2::LEFT_CENTER, *name, font(FS), fg);
        }
        if st.coll_edit {
            // 13 px checkbox 8 px from the separator: orange ✓ = shown.
            let cb = Rect::from_min_size(pos2(r.right() - 21.0, r.top() + 2.0), vec2(13.0, 13.0));
            sp.rect_filled(cb, 0.0, gray(0x11));
            sp.rect_filled(cb.shrink(1.0), 0.0, if shown { rgb([0xff, 0xad, 0x56]) } else { gray(0x1e) });
            if shown {
                let c = cb.center();
                sp.add(egui::Shape::line(
                    vec![c + vec2(-3.0, 0.0), c + vec2(-1.0, 2.5), c + vec2(3.5, -3.0)],
                    Stroke::new(1.4, gray(0x17)),
                ));
            }
            if resp.clicked() {
                app.settings.browser.hidden ^= 1 << i;
                app.save_settings();
            }
        } else if resp.clicked() {
            go(app, &mut st, Cat::Collection(i as u8), app.browser_dir.clone());
        }
        if !wide {
            resp.on_hover_text(*name);
        }
    }
    // Library.
    let lib_label = y + 25.0;
    label(lib_label, "Lib.", "Library");
    let side_item = |ui: &mut egui::Ui, st: &State, y: f32, k: (&str, usize), ic: Ico, name: &str, enabled: bool, sel: bool| {
        let r = Rect::from_min_size(at(0.0, y), vec2(side_w, 18.0));
        let resp = ui.interact(r, ui.make_persistent_id(k), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && sel {
            sp.rect_filled(r, 0.0, sel_bg);
        } else if enabled && resp.hovered() {
            sp.rect_filled(r, 0.0, gray(0x40));
        }
        let col = if !enabled {
            gray(0x5a)
        } else if sel {
            sel_fg
        } else {
            text()
        };
        icon(&sp, ic, pos2(r.left() + 13.5, r.center().y), col);
        if wide {
            sp.text(pos2(r.left() + 25.0, r.center().y), Align2::LEFT_CENTER, name, font(FS), col);
        }
        let _ = st;
        let tip = if enabled { name.to_string() } else { format!("{name} (not available in this version)") };
        if wide && enabled { resp } else { resp.on_hover_text(tip) }
    };
    let mut y = lib_label + 10.0;
    for (i, (ic, name, cat)) in LIB.iter().enumerate() {
        let enabled = *cat != Cat::Other;
        if side_item(ui, &st, y, ("brw_lib", i), *ic, name, enabled, st.cat == *cat).clicked() {
            go(app, &mut st, *cat, app.browser_dir.clone());
        }
        y += 19.0;
    }
    // Places.
    let pl_label = y + 28.0;
    label(pl_label, "Pl.", "Places");
    let home =
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let proj_dir = app.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf));
    let home_name = home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Home".into());
    let in_files = st.cat == Cat::Files;
    let places: [(Ico, &str, Option<PathBuf>); 5] = [
        (Ico::Packs, "Packs", None),
        (Ico::User, "User Library", None),
        (Ico::Project, "Current Project", proj_dir.clone()),
        (Ico::Folder, &home_name, Some(home.clone())),
        (Ico::AddFolder, "Add Folder...", None),
    ];
    let mut y = pl_label + 10.0;
    for (i, (ic, name, dir)) in places.iter().enumerate() {
        let sel = in_files
            && dir.as_ref().is_some_and(|d| app.browser_dir.starts_with(d))
            && !(i == 3 && proj_dir.as_ref().is_some_and(|d| app.browser_dir.starts_with(d) && d.starts_with(&home)));
        if side_item(ui, &st, y, ("brw_place", i), *ic, name, dir.is_some(), sel).clicked()
            && let Some(d) = dir
        {
            go(app, &mut st, Cat::Files, d.clone());
        }
        y += 19.0;
    }

    // Filter view (optional, greyed chips), then the Name header.
    let lx = side.right() + 2.0;
    let mut hdr_y = HDR_Y;
    if st.filters {
        let filt = Rect::from_min_max(at(lx - full.left(), 23.0), at(w, 76.0));
        let dimc = gray(0x62);
        p.text(filt.left_top() + vec2(5.0, 10.0), Align2::LEFT_CENTER, "Content ▾", font(FS), dimc);
        let mut x = filt.left() + 4.0;
        let mut fy = filt.top() + 20.0;
        for chip in ["Backup Set", "Device", "Groove", "MIDI Clip", "MIDI Tool", "Preset", "Sample", "Set", "Tuning"] {
            let g = p.layout_no_wrap(chip.to_string(), font(FS), dimc);
            let cw = g.size().x + 8.0;
            if x + cw > filt.right() - 4.0 {
                x = filt.left() + 4.0;
                fy += 16.0;
            }
            let r = Rect::from_min_size(pos2(x, fy), vec2(cw, 13.0));
            p.rect_filled(r, 0.0, gray(0x40));
            p.galley(pos2(r.left() + 4.0, r.center().y - g.size().y / 2.0), g, dimc);
            x += cw + 3.0;
        }
        ui.interact(filt, ui.make_persistent_id("brw_filter"), Sense::hover()).on_hover_text("Filters (not available in this version)");
        hfill(Rect::from_min_max(pos2(lx, full.top() + 76.0), at(w, 78.0)));
        hdr_y = 78.0;
    }
    let hdr = Rect::from_min_max(pos2(lx, full.top() + hdr_y), at(w, hdr_y + 19.0));
    p.rect_filled(hdr, 0.0, gray(0x46));
    let hp = p.with_clip_rect(hdr);
    hp.text(hdr.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, "Name", bold(FS), text());
    let mut name_r = hdr.right();
    for (i, (n, cw)) in COLS.iter().enumerate().rev() {
        if st.cols & (1 << i) != 0 {
            name_r -= cw;
            let _ = n;
        }
    }
    let mut x = name_r;
    for (i, (n, cw)) in COLS.iter().enumerate() {
        if st.cols & (1 << i) != 0 {
            hp.rect_filled(Rect::from_min_max(pos2(x, hdr.top() + 2.0), pos2(x + 1.0, hdr.bottom() - 2.0)), 0.0, dim());
            hp.text(pos2(x + 5.0, hdr.center().y), Align2::LEFT_CENTER, *n, bold(FS), text());
            x += cw;
        }
    }
    let ax = if st.cols != 0 { name_r - 8.0 } else { hdr.right() - 24.0 };
    let a = pos2(ax, hdr.center().y);
    hp.add(egui::Shape::convex_polygon(vec![a + vec2(0.0, -3.5), a + vec2(3.5, 3.0), a + vec2(-3.5, 3.0)], gray(0x6a), Stroke::NONE));
    // 3-dots button: extra columns and file extensions.
    let dc = pos2(hdr.right() - 9.0, hdr.center().y);
    let dresp = ui.interact(Rect::from_center_size(dc, vec2(14.0, 14.0)), ui.make_persistent_id("brw_dots"), Sense::click());
    hp.circle_filled(dc, 6.0, gray(0x1f));
    for i in -1..=1 {
        hp.circle_filled(dc + vec2(i as f32 * 3.0, 0.0), 0.9, text());
    }
    let mut prefs = app.settings.browser.clone();
    egui::Popup::menu(&dresp)
        .frame(egui::Frame::new().fill(gray(0x24)).stroke(Stroke::new(1.0, menu::line())).inner_margin(0))
        .width(134.0)
        .align(egui::RectAlign::BOTTOM_START)
        .gap(1.0)
        .show(|ui| {
            ui.set_width(132.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.add_space(1.0);
            for (i, (n, _)) in COLS.iter().enumerate() {
                if menu::check_item(ui, n, "", prefs.columns & (1 << i) != 0, true) {
                    prefs.columns ^= 1 << i;
                }
            }
            menu::sep(ui);
            if menu::item(ui, "Hide All Extra Columns", "", prefs.columns != 0) {
                prefs.columns = 0;
            }
            menu::sep(ui);
            if menu::check_item(ui, "Show File Extensions", "", prefs.show_extensions, true) {
                prefs.show_extensions = !prefs.show_extensions;
            }
            ui.add_space(1.0);
        });
    if prefs != app.settings.browser {
        app.settings.browser = prefs;
        app.save_settings();
    }

    // Footer: 1 px separator, 21 px row with the Preview switch.
    let foot = Rect::from_min_max(pos2(lx, full.bottom() - FOOT_H), full.max);
    p.rect_filled(Rect::from_min_max(pos2(lx, foot.top() - 1.0), pos2(full.right(), foot.top())), 0.0, sep());
    let hpr = Rect::from_center_size(pos2(foot.left() + 11.0, foot.center().y), vec2(14.0, 14.0));
    let resp = ui
        .interact(hpr, ui.make_persistent_id("brw_preview"), Sense::click())
        .on_hover_text("Preview: click a sample to hear it on the Cue bus");
    let on = app.preview_on;
    p.circle_filled(hpr.center(), 6.5, if on { Color32::from_rgb(0x1e, 0x5f, 0xb4) } else { gray(0x24) });
    let hc = if on { Color32::from_rgb(0x0e, 0x1e, 0x33) } else { gray(0x80) };
    p.circle_stroke(hpr.center() + vec2(0.0, 0.5), 3.5, Stroke::new(1.3, hc));
    p.rect_filled(Rect::from_center_size(hpr.center() + vec2(-3.5, 2.0), vec2(2.0, 3.0)), 0.0, hc);
    p.rect_filled(Rect::from_center_size(hpr.center() + vec2(3.5, 2.0), vec2(2.0, 3.0)), 0.0, hc);
    if resp.clicked() {
        app.preview_on = !on;
        if on {
            app.stop_preview();
        }
    }

    // List.
    let list = Rect::from_min_max(pos2(lx, hdr.bottom()), pos2(full.right(), foot.top() - 1.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(list);
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let q = st.search.trim().to_lowercase();
    let salt = format!("{:?}", st.cat);
    egui::ScrollArea::vertical().id_salt(("browser", salt)).auto_shrink([false, false]).show(&mut child, |ui| {
        if !q.is_empty() {
            // Flat search results over the current category.
            match st.cat {
                Cat::Files => files(app, ui, &mut st, 0, &q),
                Cat::Collection(_) => collection(app, ui, &mut st, &q),
                _ => {
                    if matches("factory kit", &q) {
                        leaf(app, ui, &mut st, 0, Ico::Rack, "Factory Kit", "kit", DragItem::DrumKit, None, LIB_INFO("Rack"));
                    }
                    sounds(app, ui, &mut st, 0, &q);
                    samples(app, ui, &mut st, 0, &q);
                }
            }
            return;
        }
        match st.cat {
            Cat::All => {
                if folder(ui, &mut st, 0, Ico::Sounds, "Sounds") {
                    sounds(app, ui, &mut st, 1, "");
                }
                if folder(ui, &mut st, 0, Ico::Drums, "Drums") {
                    leaf(app, ui, &mut st, 1, Ico::Rack, "Factory Kit", "kit", DragItem::DrumKit, None, LIB_INFO("Rack"));
                }
                if folder(ui, &mut st, 0, Ico::Instruments, "Instruments") {
                    instruments(app, ui, &mut st, 1);
                }
                if folder(ui, &mut st, 0, Ico::Samples, "Samples") {
                    samples(app, ui, &mut st, 1, "");
                }
                if folder(ui, &mut st, 0, Ico::Folder, "Files") {
                    files(app, ui, &mut st, 1, "");
                }
            }
            Cat::Sounds => sounds(app, ui, &mut st, 0, ""),
            Cat::Drums => {
                leaf(app, ui, &mut st, 0, Ico::Rack, "Factory Kit", "kit", DragItem::DrumKit, None, LIB_INFO("Rack"));
                samples(app, ui, &mut st, 0, "");
            }
            Cat::Instruments => instruments(app, ui, &mut st, 0),
            Cat::Samples => samples(app, ui, &mut st, 0, ""),
            Cat::Files => files(app, ui, &mut st, 0, ""),
            Cat::Collection(_) => collection(app, ui, &mut st, ""),
            Cat::Other => {}
        }
    });
    ui.data_mut(|d| d.insert_temp(id, st));
}

/// Items tagged with the selected collection.
fn collection(app: &mut DawApp, ui: &mut egui::Ui, st: &mut State, q: &str) {
    let Cat::Collection(i) = st.cat else { return };
    let keys: Vec<String> = app.settings.browser.collections.iter().filter(|(_, m)| **m & (1 << i) != 0).map(|(k, _)| k.clone()).collect();
    for k in keys {
        let name = k.rsplit(['/', '\\', ':']).next().unwrap_or(&k).to_string();
        if matches(&name, q) {
            keyed_item(app, ui, st, &k);
        }
    }
}

/// Keys 1-7 toggle the selected item's collections while the browser has focus
/// (plain keys arrive via `key_events`, so this runs before the shortcuts).
pub fn collection_keys(app: &mut DawApp, ctx: &egui::Context) {
    let focused = ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("focus_browser"))).unwrap_or(false);
    let Some(key) = ctx.data(|d| d.get_temp::<State>(egui::Id::unique("browser_state"))).and_then(|s| s.selected) else { return };
    if !focused || !app.show_browser {
        return;
    }
    let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7];
    let mut hits = Vec::new();
    app.key_events.retain(|e| match e {
        egui::Event::Key { key, pressed, modifiers, .. } if !modifiers.any() && digits.contains(key) => {
            if *pressed {
                hits.push(digits.iter().position(|d| d == key).unwrap_or(0));
            }
            false
        }
        _ => true,
    });
    for i in hits {
        toggle_collection(app, &key, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_sizes() {
        assert_eq!(fmt_date(0), "1970-01-01 00:00");
        assert_eq!(fmt_date(1_793_232_000 + 3723), "2026-10-29 01:02");
        assert_eq!(fmt_size(99_000), "97 KB");
        assert_eq!(fmt_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn search_is_case_insensitive() {
        assert!(matches("Square Lead", "lead"));
        assert!(matches("Pad", ""));
        assert!(!matches("Bass", "pa"));
    }
}
