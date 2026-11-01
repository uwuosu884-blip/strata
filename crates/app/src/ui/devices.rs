//! Audio effect devices in the device view: Live's device frame, Utility and
//! Align Delay. Geometry measured from Live 12 screenshots at 100 % (T/L =
//! device top/left; text positions are baselines).

use super::menu;
use super::theme::*;
use crate::app::{DawApp, DragItem};
use crate::fx_ops::{ChainRef, chain};
use daw_project::Device;
use daw_project::devices::*;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, pos2, vec2};
use std::f32::consts::PI;

const TITLE_H: f32 = 17.0;
/// Space between devices: 2 px, grip (2 + 1 + 2), 2 px.
/// Device chain background grey.
pub const CHAIN_BG: u8 = 0x2a;
pub const GAP: f32 = 9.0;
const TEAL: Color32 = Color32::from_rgb(3, 195, 213);
const BOX_TEAL: Color32 = VOL_TEAL;
/// Knob drag: pixels for the full range.
const DRAG_PX: f32 = 200.0;

pub fn width(d: &Device) -> f32 {
    match d.params {
        FxParams::Utility(_) => 139.0,
        FxParams::AlignDelay(_) => 112.0,
    }
}

// ----------------------------------------------------------------------------
// Parameters: normalised values, defaults, display, steps.
// ----------------------------------------------------------------------------

const P18: f32 = 0.5 - 18.0 / 70.0;

/// Utility Gain knob: 0 dB at the top, 70 dB per knob turn above −18 dB,
/// a quadratic amplitude taper down to −inf below.
pub fn gain_to_norm(db: f32) -> f32 {
    if db <= UTIL_GAIN_MIN_DB {
        0.0
    } else if db >= -18.0 {
        (0.5 + db / 70.0).min(1.0)
    } else {
        P18 * 10f32.powf((db + 18.0) / 40.0)
    }
}

pub fn norm_to_gain(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    let db = if p >= P18 {
        70.0 * (p - 0.5)
    } else if p <= 0.0 {
        UTIL_GAIN_MIN_DB
    } else {
        -18.0 + 40.0 * (p / P18).log10()
    };
    if db <= UTIL_GAIN_MIN_DB + 0.05 { UTIL_GAIN_MIN_DB } else { db.min(UTIL_GAIN_MAX_DB) }
}

fn fmt_db(db: f32) -> String {
    if db <= UTIL_GAIN_MIN_DB {
        "-inf dB".into()
    } else if db.abs() < 9.995 {
        format!("{db:.2} dB")
    } else {
        format!("{db:.1} dB")
    }
}

/// Utility parameter ids.
pub mod up {
    pub const PHASE_L: u8 = 0;
    pub const PHASE_R: u8 = 1;
    pub const CHANNEL: u8 = 2;
    pub const WIDTH: u8 = 3;
    pub const MONO: u8 = 4;
    pub const BASS: u8 = 5;
    pub const FREQ: u8 = 6;
    pub const AUDITION: u8 = 7;
    pub const GAIN: u8 = 8;
    pub const BALANCE: u8 = 9;
    pub const MUTE: u8 = 10;
    pub const DC: u8 = 11;
}

/// Align Delay parameter ids.
pub mod ap {
    pub const MODE: u8 = 0;
    pub const LINK: u8 = 1;
    pub const LEFT: u8 = 2;
    pub const RIGHT: u8 = 3;
    pub const FEET: u8 = 4;
    pub const TEMP: u8 = 5;
    pub const FAHRENHEIT: u8 = 6;
}

/// Continuous parameters: normalised value (None for switches).
pub fn norm(p: &FxParams, k: u8) -> Option<f32> {
    match p {
        FxParams::Utility(u) => Some(match k {
            up::WIDTH if u.ms_mode => (u.mid_side + 1.0) / 2.0,
            up::WIDTH if u.width <= 1.0 => u.width / 2.0,
            up::WIDTH => 0.5 + (u.width - 1.0) / 6.0,
            up::FREQ => (u.bass_freq / BASS_MONO_MIN_HZ).ln() / 10f32.ln(),
            up::GAIN => gain_to_norm(u.gain_db),
            up::BALANCE => (u.balance + 1.0) / 2.0,
            _ => return None,
        }),
        FxParams::AlignDelay(a) => Some(match k {
            ap::LEFT | ap::RIGHT => (align_value(a, k == ap::RIGHT) / align_max(a)).max(0.0).sqrt(),
            ap::TEMP => (a.temp - ALIGN_TEMP_MIN_C) / (ALIGN_TEMP_MAX_C - ALIGN_TEMP_MIN_C),
            _ => return None,
        }),
    }
}

pub fn set_norm(p: &mut FxParams, k: u8, v: f32) {
    let v = v.clamp(0.0, 1.0);
    match p {
        FxParams::Utility(u) => match k {
            up::WIDTH if u.ms_mode => u.mid_side = v * 2.0 - 1.0,
            up::WIDTH => u.width = if v <= 0.5 { v * 2.0 } else { 1.0 + (v - 0.5) * 6.0 },
            up::FREQ => u.bass_freq = BASS_MONO_MIN_HZ * 10f32.powf(v),
            up::GAIN => u.gain_db = norm_to_gain(v),
            up::BALANCE => u.balance = v * 2.0 - 1.0,
            _ => {}
        },
        FxParams::AlignDelay(a) => match k {
            ap::LEFT | ap::RIGHT => {
                let x = v * v * align_max(a);
                set_align_value(a, k == ap::RIGHT, x);
            }
            ap::TEMP => a.temp = ALIGN_TEMP_MIN_C + v * (ALIGN_TEMP_MAX_C - ALIGN_TEMP_MIN_C),
            _ => {}
        },
    }
}

fn align_max(a: &AlignDelayParams) -> f32 {
    match a.mode {
        DelayMode::Time => ALIGN_TIME_MAX_MS,
        DelayMode::Samples => ALIGN_SAMPLES_MAX,
        DelayMode::Distance => ALIGN_DIST_MAX_M,
    }
}

/// Current L/R value in the mode's unit (metres for Distance).
fn align_value(a: &AlignDelayParams, right: bool) -> f32 {
    let right = right && !a.link;
    match (a.mode, right) {
        (DelayMode::Time, false) => a.time_l,
        (DelayMode::Time, true) => a.time_r,
        (DelayMode::Samples, false) => a.samples_l,
        (DelayMode::Samples, true) => a.samples_r,
        (DelayMode::Distance, false) => a.dist_l,
        (DelayMode::Distance, true) => a.dist_r,
    }
}

fn set_align_value(a: &mut AlignDelayParams, right: bool, x: f32) {
    let x = x.clamp(0.0, align_max(a));
    match (a.mode, right) {
        (DelayMode::Time, false) => a.time_l = x,
        (DelayMode::Time, true) => a.time_r = x,
        (DelayMode::Samples, false) => a.samples_l = x.round(),
        (DelayMode::Samples, true) => a.samples_r = x.round(),
        (DelayMode::Distance, false) => a.dist_l = x,
        (DelayMode::Distance, true) => a.dist_r = x,
    }
}

/// Live's "Return to Default" (Delete on a focused parameter).
pub fn reset(p: &mut FxParams, k: u8) {
    match p {
        FxParams::Utility(u) => {
            let d = UtilityParams::default();
            match k {
                up::PHASE_L => u.phase_l = false,
                up::PHASE_R => u.phase_r = false,
                up::CHANNEL => u.channel = d.channel,
                up::WIDTH if u.ms_mode => u.mid_side = 0.0,
                up::WIDTH => u.width = 1.0,
                up::MONO => u.mono = false,
                up::BASS => u.bass_mono = false,
                up::FREQ => u.bass_freq = d.bass_freq,
                up::AUDITION => u.bass_audition = false,
                up::GAIN => u.gain_db = 0.0,
                up::BALANCE => u.balance = 0.0,
                up::MUTE => u.mute = false,
                up::DC => u.dc = false,
                _ => {}
            }
        }
        FxParams::AlignDelay(a) => match k {
            ap::MODE => a.mode = DelayMode::Time,
            ap::LINK => a.link = false,
            ap::LEFT => set_align_value(a, false, 0.0),
            ap::RIGHT => set_align_value(a, true, 0.0),
            ap::FEET => a.feet = false,
            ap::TEMP => a.temp = 20.0,
            ap::FAHRENHEIT => a.fahrenheit = false,
            _ => {}
        },
    }
}

/// Arrow keys on a focused parameter (`dir` = ±1). Gain steps 1 dB down to
/// −18 dB and accelerates below (Live).
pub fn step(p: &mut FxParams, k: u8, dir: f32) {
    match p {
        FxParams::Utility(u) => match k {
            up::GAIN => {
                if u.gain_db > -18.0 + 1e-3 || (dir > 0.0 && u.gain_db >= -18.0 - 1e-3) {
                    u.gain_db = (u.gain_db.round() + dir).clamp(-18.0, UTIL_GAIN_MAX_DB);
                } else {
                    let n = (gain_to_norm(u.gain_db) + dir * 0.02).clamp(0.0, P18);
                    u.gain_db = if dir > 0.0 && n >= P18 - 1e-4 { -18.0 } else { norm_to_gain(n) };
                }
            }
            up::WIDTH if u.ms_mode => u.mid_side = ((u.mid_side * 100.0).round() + dir).clamp(-100.0, 100.0) / 100.0,
            up::WIDTH => u.width = ((u.width * 100.0).round() + dir).clamp(0.0, UTIL_WIDTH_MAX * 100.0) / 100.0,
            up::BALANCE => u.balance = ((u.balance * 50.0).round() + dir).clamp(-50.0, 50.0) / 50.0,
            up::FREQ => u.bass_freq = (u.bass_freq.round() + dir).clamp(BASS_MONO_MIN_HZ, BASS_MONO_MAX_HZ),
            up::CHANNEL => {
                let i = ChannelMode::ALL.iter().position(|&c| c == u.channel).unwrap_or(2) as i32 - dir as i32;
                u.channel = ChannelMode::ALL[i.clamp(0, 3) as usize];
            }
            _ => {}
        },
        FxParams::AlignDelay(a) => match k {
            ap::LEFT | ap::RIGHT => {
                let unit = if a.mode == DelayMode::Samples { 1.0 } else { 0.01 };
                let unit = if a.mode == DelayMode::Distance && a.feet { unit / FEET_PER_M } else { unit };
                let r = k == ap::RIGHT;
                let v = align_value(a, r) + dir * unit;
                set_align_value(a, r, v);
            }
            ap::TEMP => {
                let unit = if a.fahrenheit { 5.0 / 9.0 * 0.01 } else { 0.01 };
                a.temp = (a.temp + dir * unit).clamp(ALIGN_TEMP_MIN_C, ALIGN_TEMP_MAX_C);
            }
            ap::MODE => {
                let i = DelayMode::ALL.iter().position(|&m| m == a.mode).unwrap_or(0) as i32 - dir as i32;
                a.mode = DelayMode::ALL[i.clamp(0, 2) as usize];
            }
            _ => {}
        },
    }
}

/// Parameter name (Copy Parameter Name; Live's names).
fn param_name(p: &FxParams, k: u8) -> &'static str {
    match p {
        FxParams::Utility(u) => match k {
            up::PHASE_L => "Left Inv",
            up::PHASE_R => "Right Inv",
            up::CHANNEL => "Channel Mode",
            up::WIDTH if u.ms_mode => "Mid/Side Balance",
            up::WIDTH => "Stereo Width",
            up::MONO => "Mono",
            up::BASS => "Bass Mono",
            up::FREQ => "Bass Freq",
            up::AUDITION => "Bass Mono Audition",
            up::GAIN => "Gain",
            up::BALANCE => "Balance",
            up::MUTE => "Mute",
            _ => "DC Filter",
        },
        FxParams::AlignDelay(_) => match k {
            ap::MODE => "Delay Mode",
            ap::LINK => "Link",
            ap::LEFT => "Left",
            ap::RIGHT => "Right",
            ap::FEET => "Distance Unit",
            ap::TEMP => "Temperature",
            _ => "Temperature Unit",
        },
    }
}

// ----------------------------------------------------------------------------
// Drawing helpers.
// ----------------------------------------------------------------------------

/// Text placed by its baseline.
fn text_at(p: &Painter, x: f32, base: f32, align: Align2, s: &str, f: FontId, col: Color32) -> Rect {
    let g = p.layout_no_wrap(s.to_string(), f, col);
    let asc = g.rows.first().map(|r| r.pos.y + r.row.glyphs.first().map(|gl| gl.pos.y).unwrap_or(r.row.size.y * 0.8)).unwrap_or(0.0);
    let w = g.size().x;
    let x0 = match align.x() {
        egui::Align::Min => x,
        egui::Align::Center => x - w / 2.0,
        egui::Align::Max => x - w,
    };
    let pos = pos2(x0.round(), (base - asc).round());
    let r = Rect::from_min_size(pos, g.size());
    p.galley(pos, g, col);
    r
}

/// Truncate `s` with "..." to fit `w`.
fn fit(p: &Painter, s: &str, f: &FontId, w: f32) -> String {
    let width = |t: &str| p.layout_no_wrap(t.to_string(), f.clone(), Color32::WHITE).size().x;
    if width(s) <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    for n in (0..chars.len()).rev() {
        let t: String = chars[..n].iter().collect::<String>() + "...";
        if width(&t) <= w {
            return t;
        }
    }
    String::new()
}

/// 1 px #111 border box with `fill`.
fn boxed(p: &Painter, r: Rect, fill: Color32) {
    p.rect_filled(r, CornerRadius::ZERO, gray(0x11));
    p.rect_filled(r.shrink(1.0), CornerRadius::ZERO, fill);
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Live text toggle (13 px tall): #1e off, orange on, greyed when disabled.
fn toggle_look(p: &Painter, r: Rect, label: &str, on: bool, enabled: bool) {
    let (fill, col) = match (on, enabled) {
        (false, true) => (gray(0x1e), text()),
        (true, true) => (ORANGE, Color32::from_rgb(0x20, 0x20, 0x20)),
        (false, false) => (gray(0x20), gray(0x69)),
        (true, false) => (mix(ORANGE, gray(0x36), 0.5), gray(0x36)),
    };
    boxed(p, r, fill);
    text_at(p, r.center().x, r.top() + 10.0, Align2::CENTER_BOTTOM, label, font(FS), col);
}

/// Knob angle (radians from 12 o'clock, clockwise) for a normalised value.
fn angle(v: f32) -> f32 {
    (-150.0 + 300.0 * v.clamp(0.0, 1.0)) * PI / 180.0
}

/// Unsnapped filled rect (egui snaps `rect_filled` to whole pixels).
fn quad(p: &Painter, r: Rect, c: Color32) {
    p.add(Shape::convex_polygon(vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], c, Stroke::NONE));
}

fn arc(p: &Painter, c: Pos2, r: f32, a0: f32, a1: f32, stroke: Stroke) {
    let (a0, a1) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
    let n = (((a1 - a0) / (3.0 * PI / 180.0)).ceil() as usize).max(1);
    let pts: Vec<Pos2> = (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|a| c + vec2(a.sin(), -a.cos()) * r).collect();
    p.add(Shape::line(pts, stroke));
}

/// Live knob: #111 track over ±150°, teal arc from the default to the value,
/// marker triangle on top (teal when off-default), dark needle.
fn knob_look(p: &Painter, c: Pos2, v: f32, def: f32, enabled: bool) {
    let changed = (v - def).abs() > 1e-4;
    let (track, hi, needle) = if enabled { (gray(0x11), TEAL, gray(0x18)) } else { (gray(0x2a), gray(0x75), gray(0x2a)) };
    // Live breaks the ring ±GAP around the needle.
    const GAP: f32 = 0.2;
    const R: f32 = 11.9;
    let w = Stroke::new(2.15, track);
    let (a, lo, top) = (angle(v), angle(0.0), angle(1.0));
    if a - GAP > lo {
        arc(p, c, R, lo, a - GAP, w);
    }
    if a + GAP < top {
        arc(p, c, R, a + GAP, top, w);
    }
    if changed {
        let d = angle(def);
        let e = if a > d { (a - GAP).max(d) } else { (a + GAP).min(d) };
        if (e - d).abs() > 1e-3 {
            arc(p, c, R, d, e, Stroke::new(2.15, hi));
        }
    }
    let tri = if changed { hi } else { track };
    p.add(Shape::convex_polygon(vec![pos2(c.x - 3.5, c.y - 18.5), pos2(c.x + 3.5, c.y - 18.5), pos2(c.x, c.y - 14.0)], tri, Stroke::NONE));
    // A path, not a line segment: egui would snap the segment to pixels.
    p.add(Shape::line(vec![c, c + vec2(a.sin(), -a.cos()) * 12.5], Stroke::new(2.1, needle)));
}

/// Focus corner brackets (4 px arms, #919191).
fn brackets(p: &Painter, r: Rect) {
    let col = gray(0x91);
    let px = |x: f32, y: f32, w: f32, h: f32| p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, h)), CornerRadius::ZERO, col);
    let (l, t, rr, b) = (r.left(), r.top(), r.right() - 1.0, r.bottom() - 1.0);
    px(l, t, 4.0, 1.0);
    px(l, t, 1.0, 4.0);
    px(rr - 3.0, t, 4.0, 1.0);
    px(rr, t, 1.0, 4.0);
    px(l, b, 4.0, 1.0);
    px(l, b - 3.0, 1.0, 4.0);
    px(rr - 3.0, b, 4.0, 1.0);
    px(rr, b - 3.0, 1.0, 4.0);
}

/// Grip between devices (and after the last one), like the chain start grip.
pub fn grip(p: &Painter, x: f32, top: f32, bottom: f32) {
    let (gy0, gy1) = (top + 40.0, bottom - 40.0);
    if gy1 <= gy0 + 12.0 {
        return;
    }
    for dx in [0.0, 3.0] {
        p.rect_filled(Rect::from_min_max(pos2(x + dx, gy0), pos2(x + dx + 2.0, gy1)), 0.0, gray(0x18));
        p.rect_filled(Rect::from_min_max(pos2(x + dx, gy0 + 11.0), pos2(x + dx + 2.0, gy0 + 12.0)), 0.0, gray(CHAIN_BG));
    }
}

// ----------------------------------------------------------------------------
// Interaction context for one device.
// ----------------------------------------------------------------------------

struct Ctx<'a> {
    app: &'a mut DawApp,
    chain: ChainRef,
    dev: Device,
    /// Edited copy; written back at the end of the frame when changed.
    params: FxParams,
    on: bool,
    gesture: bool,
}

impl<'a> Ctx<'a> {
    fn focused(&self, k: u8) -> bool {
        self.app.dev_focus == Some((self.dev.id, k))
    }

    fn focus(&mut self, k: u8) {
        self.app.dev_focus = Some((self.dev.id, k));
        self.app.dev_sel = Some(self.dev.id);
        self.app.focus_detail = true;
    }

    /// Start an undo step (once per frame).
    fn undo_step(&mut self) {
        if !self.gesture {
            self.app.begin_gesture();
            self.gesture = true;
        }
    }

    fn commit(self) -> &'a mut DawApp {
        if self.params != self.dev.params || self.on != self.dev.on {
            self.app.set_device(self.chain, self.dev.id, self.on, self.params);
        }
        self.app
    }

    fn sense(&self, ui: &egui::Ui, r: Rect, k: u8, s: Sense) -> Response {
        ui.interact(r, egui::Id::unique(("devp", self.dev.id, k)), s)
    }

    /// Click toggle: focus + flip.
    fn toggle(&mut self, ui: &egui::Ui, r: Rect, k: u8, enabled: bool) -> bool {
        let resp = self.sense(ui, r, k, Sense::click());
        self.param_menu(&resp, k);
        if resp.clicked() {
            self.focus(k);
            if enabled {
                self.undo_step();
                return true;
            }
        }
        false
    }

    /// Vertical drag on a continuous parameter; returns the response.
    fn drag(&mut self, ui: &egui::Ui, r: Rect, k: u8, enabled: bool) -> Response {
        let resp = self.sense(ui, r, k, Sense::click_and_drag());
        if resp.drag_started() || resp.clicked() {
            self.focus(k);
        }
        if enabled && resp.dragged() {
            let dy = resp.drag_delta().y;
            if dy != 0.0 {
                self.undo_step();
                let fine = ui.input(|i| i.modifiers.shift);
                let v = norm(&self.params, k).unwrap_or(0.0) - dy / if fine { DRAG_PX * 10.0 } else { DRAG_PX };
                set_norm(&mut self.params, k, v);
            }
        }
        self.param_menu(&resp, k);
        resp
    }

    /// Parameter context menu (Live): automation / mapping entries greyed.
    fn param_menu(&mut self, resp: &Response, k: u8) {
        let width_knob = matches!(self.params, FxParams::Utility(_)) && k == up::WIDTH;
        let mut ms_toggle = false;
        let mut to_default = false;
        let mut copy_name = false;
        menu::context(resp, 223.0, |ui| {
            if width_knob {
                let ms = matches!(self.params, FxParams::Utility(u) if u.ms_mode);
                ms_toggle = menu::check_item(ui, "Mid/Side Mode", "", ms, true);
                menu::sep(ui);
            }
            menu::item(ui, "Show Automation", "", false);
            menu::item(ui, "Show Automation In New Lane", "", false);
            menu::sep(ui);
            menu::item(ui, "Show Modulation Source 1: Clip Envelope", "", false);
            menu::sep(ui);
            to_default = menu::item(ui, "Return to Default", "Del", true);
            menu::sep(ui);
            menu::item(ui, "Edit MIDI Map", "Ctrl+M", false);
            menu::item(ui, "Edit Key Map", "Ctrl+K", false);
            menu::sep(ui);
            menu::item(ui, "Copy Max for Live Path", "", false);
            copy_name = menu::item(ui, "Copy Parameter Name", "", true);
        });
        if resp.secondary_clicked() {
            self.focus(k);
        }
        if ms_toggle && matches!(self.params, FxParams::Utility(_)) {
            self.undo_step();
            if let FxParams::Utility(u) = &mut self.params {
                u.ms_mode = !u.ms_mode;
            }
        }
        if to_default {
            self.undo_step();
            reset(&mut self.params, k);
        }
        if copy_name {
            resp.ctx.copy_text(param_name(&self.params, k).to_string());
        }
    }
}

// ----------------------------------------------------------------------------
// Device frame + chain.
// ----------------------------------------------------------------------------

/// Draw device `idx` of chain `c` in `r` (full height).
pub fn device(app: &mut DawApp, ui: &mut egui::Ui, r: Rect, c: ChainRef, idx: usize) {
    let Some(&dev) = chain(&app.project, c).get(idx) else { return };
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let (l, t) = (r.left(), r.top());
    p.rect_filled(r, CornerRadius::same(3), panel());
    let title = Rect::from_min_size(r.min, vec2(r.width(), TITLE_H));
    p.rect_filled(title, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, gray(0x46));
    let mut cx = Ctx { app, chain: c, dev, params: dev.params, on: dev.on, gesture: false };

    // Title strip: select / drag / context menu.
    let tr = ui.interact(title, egui::Id::unique(("dev_title", dev.id)), Sense::click_and_drag());
    if tr.clicked() || tr.drag_started() || tr.secondary_clicked() {
        cx.app.dev_sel = Some(dev.id);
        cx.app.dev_focus = None;
        cx.app.focus_detail = true;
    }
    tr.dnd_set_drag_payload(DragItem::ChainDevice { chain: c, id: dev.id });
    if tr.dragged() {
        super::cursors::set(ui.ctx(), super::cursors::Cur::Drag);
    }
    let mut delete = false;
    let mut dup = false;
    menu::context(&tr, 223.0, |ui| {
        menu::item(ui, "Cut", "Ctrl+X", false);
        menu::item(ui, "Copy", "Ctrl+C", false);
        dup = menu::item(ui, "Duplicate", "Ctrl+D", true);
        delete = menu::item(ui, "Delete", "Del", true);
        menu::sep(ui);
        menu::item(ui, "Rename", "Ctrl+R", false);
        menu::item(ui, "Edit Info Text", "", false);
        menu::sep(ui);
        menu::item(ui, "Group", "Ctrl+G", false);
        menu::item(ui, "Group to Audio Effect Rack", "", false);
        menu::sep(ui);
        menu::item(ui, "Fold", "", false);
        menu::item(ui, "Show Preset Name", "", false);
        menu::sep(ui);
        menu::item(ui, "Save as Default Preset", "", false);
        menu::item(ui, "Lock to Control Surface", "", false);
    });

    // Activator.
    let ac = pos2(l + 9.5, t + 8.5);
    p.circle_filled(ac, 6.5, gray(0x11));
    p.circle_filled(ac, 5.5, if cx.on { ORANGE } else { gray(0x46) });
    let ar = ui.interact(Rect::from_center_size(ac, vec2(13.0, 13.0)), egui::Id::unique(("dev_on", dev.id)), Sense::click());
    if ar.clicked() {
        cx.undo_step();
        cx.on = !cx.on;
    }
    // Header buttons (hot-swap, save, menu): not implemented, greyed.
    let right = r.right();
    let glyph = gray(0x75);
    for (i, dx) in [42.5, 26.5, 10.5].into_iter().enumerate() {
        let bc = pos2(right - dx, t + 8.5);
        p.circle_filled(bc, 6.5, gray(0x1e));
        match i {
            0 => {
                // Hot-swap: two curved arrows.
                arc(&p, bc, 3.2, -2.6, -0.3, Stroke::new(1.2, glyph));
                arc(&p, bc, 3.2, 0.55, 2.85, Stroke::new(1.2, glyph));
                p.add(Shape::convex_polygon(vec![bc + vec2(1.2, -4.8), bc + vec2(3.8, -2.4), bc + vec2(0.6, -1.6)], glyph, Stroke::NONE));
                p.add(Shape::convex_polygon(vec![bc + vec2(-1.2, 4.8), bc + vec2(-3.8, 2.4), bc + vec2(-0.6, 1.6)], glyph, Stroke::NONE));
            }
            1 => {
                // Save: floppy.
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
    let mut name_right = right - 49.0 - 2.0;
    if matches!(dev.params, FxParams::AlignDelay(_)) {
        // Max for Live edit icon (greyed).
        let ir = Rect::from_min_size(pos2(right - 60.0, t + 4.0), vec2(9.0, 9.0));
        let s = Stroke::new(1.0, glyph);
        p.rect_stroke(
            Rect::from_min_max(ir.min + vec2(0.5, 1.5), ir.max - vec2(0.5, 1.5)),
            CornerRadius::ZERO,
            s,
            egui::StrokeKind::Middle,
        );
        p.line_segment([pos2(ir.left(), ir.top() + 2.0), pos2(ir.right(), ir.top() + 2.0)], s);
        for x in [ir.left() + 2.5, ir.right() - 2.5] {
            p.line_segment([pos2(x, ir.top()), pos2(x, ir.top() + 1.5)], s);
            p.line_segment([pos2(x, ir.bottom() - 1.5), pos2(x, ir.bottom())], s);
        }
        name_right = ir.left() - 2.0;
    }
    let nf = bold(FS);
    let name = fit(&p, dev.params.name(), &nf, name_right - (l + 20.0));
    text_at(&p, l + 20.0, t + 12.0, Align2::LEFT_BOTTOM, &name, nf, if cx.on { text() } else { dim() });

    match dev.params {
        FxParams::Utility(_) => utility(&mut cx, ui, &p, l, t),
        FxParams::AlignDelay(_) => align_delay(&mut cx, ui, &p, l, t),
    }
    let app = cx.commit();
    if dup {
        app.duplicate_device(c, dev.id);
    }
    if delete {
        app.delete_device(c, dev.id);
    }
}

// ----------------------------------------------------------------------------
// Utility (140 px).
// ----------------------------------------------------------------------------

fn utility(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Utility(u0) = cx.params else { return };
    let (ci, co) = (l + 34.5, l + 104.5); // column centres
    // Column divider with its ▸.
    p.rect_filled(Rect::from_min_max(pos2(l + 69.0, t + 21.0), pos2(l + 70.0, t + 183.0)), CornerRadius::ZERO, gray(0x24));
    p.add(Shape::convex_polygon(
        vec![pos2(l + 69.0, t + 98.0), pos2(l + 73.0, t + 101.5), pos2(l + 69.0, t + 105.0)],
        gray(0x24),
        Stroke::NONE,
    ));
    text_at(p, ci, t + 31.0, Align2::CENTER_BOTTOM, "Input", font(FS), text());
    text_at(p, co, t + 31.0, Align2::CENTER_BOTTOM, "Output", font(FS), text());

    // Phase.
    let rl = Rect::from_min_size(pos2(l + 6.0, t + 38.0), vec2(27.0, 13.0));
    let rr = Rect::from_min_size(pos2(l + 36.0, t + 38.0), vec2(27.0, 13.0));
    toggle_look(p, rl, "Ø L", u0.phase_l, true);
    toggle_look(p, rr, "Ø R", u0.phase_r, true);
    if cx.toggle(ui, rl, up::PHASE_L, true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.phase_l = !u.phase_l;
    }
    if cx.toggle(ui, rr, up::PHASE_R, true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.phase_r = !u.phase_r;
    }

    // Channel Mode chooser.
    let dd = Rect::from_min_size(pos2(l + 6.0, t + 54.0), vec2(57.0, 13.0));
    if let Some(i) = chooser(cx, ui, p, dd, up::CHANNEL, u0.channel.label(), &ChannelMode::ALL.map(|c| c.label()), true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.channel = ChannelMode::ALL[i];
    }

    // Width / Mid-Side knob: disabled by Mono and the Left / Right modes.
    let stereo_on = !u0.mono && !matches!(u0.channel, ChannelMode::Left | ChannelMode::Right);
    let (label, value) = if u0.ms_mode {
        let r = (u0.mid_side * 100.0).round() as i32;
        let v = match r.cmp(&0) {
            std::cmp::Ordering::Less => format!("{}M", -r),
            std::cmp::Ordering::Equal => "0".into(),
            std::cmp::Ordering::Greater => format!("{r}S"),
        };
        ("Mid/Side", v)
    } else {
        ("Width", format!("{:.0} %", u0.width * 100.0))
    };
    knob_param(cx, ui, p, ci, t + 83.0, up::WIDTH, label, &value, 0.5, stereo_on);

    // Mono, Bass Mono (disabled by Mono), frequency, audition.
    let mono = Rect::from_min_size(pos2(l + 6.0, t + 131.0), vec2(57.0, 13.0));
    toggle_look(p, mono, "Mono", u0.mono, true);
    if cx.toggle(ui, mono, up::MONO, true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.mono = !u.mono;
    }
    let bass = Rect::from_min_size(pos2(l + 6.0, t + 153.0), vec2(57.0, 13.0));
    toggle_look(p, bass, "Bass Mono", u0.bass_mono, !u0.mono);
    if cx.toggle(ui, bass, up::BASS, !u0.mono)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.bass_mono = !u.bass_mono;
    }
    let fr = Rect::from_min_size(pos2(l + 6.0, t + 169.0), vec2(41.0, 13.0));
    boxed(p, fr, panel());
    text_at(p, fr.center().x, fr.top() + 10.0, Align2::CENTER_BOTTOM, &format!("{:.0} Hz", u0.bass_freq), font(FS), text());
    cx.drag(ui, fr, up::FREQ, true);
    let hc = pos2(l + 56.5, t + 175.5);
    let aud = u0.bass_audition;
    // #111 rim, #1E (or orange) face, headphone glyph.
    p.circle_filled(hc, 6.0, gray(0x11));
    p.circle_filled(hc, 5.0, if aud { ORANGE } else { gray(0x1e) });
    let gcol = if aud { Color32::from_rgb(0x20, 0x20, 0x20) } else { gray(0xaa) };
    arc(p, hc, 3.55, -PI / 2.0 - 0.28, PI / 2.0 + 0.28, Stroke::new(1.2, gcol));
    for dx in [-3.05, 1.05] {
        quad(p, Rect::from_min_size(hc + vec2(dx, 1.85), vec2(2.0, 2.0)), gcol);
    }
    if cx.toggle(ui, Rect::from_center_size(hc, vec2(13.0, 13.0)), up::AUDITION, true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.bass_audition = !u.bass_audition;
    }

    // Output: Gain, Balance, Mute, DC.
    knob_param(cx, ui, p, co, t + 53.0, up::GAIN, "Gain", &fmt_db(u0.gain_db), 0.5, true);
    let b = (u0.balance * 50.0).round() as i32;
    let bal = match b.cmp(&0) {
        std::cmp::Ordering::Less => format!("{}L", -b),
        std::cmp::Ordering::Equal => "C".into(),
        std::cmp::Ordering::Greater => format!("{b}R"),
    };
    knob_param(cx, ui, p, co, t + 115.0, up::BALANCE, "Balance", &bal, 0.5, true);
    let mr = Rect::from_min_size(pos2(l + 78.0, t + 168.0), vec2(31.0, 13.0));
    let dr = Rect::from_min_size(pos2(l + 112.0, t + 168.0), vec2(19.0, 13.0));
    toggle_look(p, mr, "Mute", u0.mute, true);
    toggle_look(p, dr, "DC", u0.dc, true);
    if cx.toggle(ui, mr, up::MUTE, true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.mute = !u.mute;
    }
    if cx.toggle(ui, dr, up::DC, true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.dc = !u.dc;
    }
}

/// Label, knob and value centred at `x` (label baseline `base`); 45 px wide
/// focus area with corner brackets.
#[allow(clippy::too_many_arguments)]
fn knob_param(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, label: &str, value: &str, def: f32, enabled: bool) {
    let area = Rect::from_min_max(pos2(x - 22.5, base - 9.0), pos2(x + 22.5, base + 45.0));
    cx.drag(ui, area, k, enabled);
    let v = norm(&cx.params, k).unwrap_or(def);
    text_at(p, x, base, Align2::CENTER_BOTTOM, label, font(FS), text());
    knob_look(p, pos2(x, base + 21.5), v, def, enabled);
    text_at(p, x, base + 42.0, Align2::CENTER_BOTTOM, value, font(FS), text());
    if cx.focused(k) {
        brackets(p, area);
    }
}

/// Live chooser (#242424, ▼ on the right); returns the picked index.
#[allow(clippy::too_many_arguments)]
fn chooser(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, cur: &str, items: &[&str], enabled: bool) -> Option<usize> {
    boxed(p, r, gray(0x24));
    let col = if enabled { text() } else { dim() };
    text_at(p, r.left() + 2.0, r.top() + 10.0, Align2::LEFT_BOTTOM, cur, font(FS), col);
    let tr = r.right();
    p.add(Shape::convex_polygon(
        vec![pos2(tr - 10.0, r.top() + 4.0), pos2(tr - 3.0, r.top() + 4.0), pos2(tr - 6.5, r.top() + 8.5)],
        col,
        Stroke::NONE,
    ));
    let resp = cx.sense(ui, r, k, Sense::click());
    if resp.clicked() {
        cx.focus(k);
    }
    let mut pick = None;
    if enabled {
        menu::dropdown_aligned(&resp, r.width().max(80.0), egui::RectAlign::BOTTOM_START, |ui| {
            for (i, it) in items.iter().enumerate() {
                if menu::check_item(ui, it, "", *it == cur, true) {
                    pick = Some(i);
                }
            }
        });
    }
    if pick.is_some() {
        cx.undo_step();
    }
    pick
}

// ----------------------------------------------------------------------------
// Align Delay (112 px).
// ----------------------------------------------------------------------------

fn align_delay(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::AlignDelay(a0) = cx.params else { return };
    let sr = cx.app.audio.info.sample_rate as f32;
    let sr = if sr > 0.0 { sr } else { 44_100.0 };

    // Read-out.
    let disp = Rect::from_min_size(pos2(l + 4.0, t + 21.0), vec2(104.0, 74.0));
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));
    let (dl, dr) = a0.delays_ms(sr);
    let dimc = gray(0x87);
    for (i, (name, ms)) in [("Left", dl), ("Right", dr)].into_iter().enumerate() {
        let base = t + 44.0 + 22.0 * i as f32;
        text_at(p, disp.left() + 6.0, base, Align2::LEFT_BOTTOM, name, font(FS), dimc);
        let col = if ms > 0.0 { ORANGE } else { dimc };
        // Max letter-spaces the unit by 1 px.
        let m = text_at(p, disp.right() - 18.0, base, Align2::LEFT_BOTTOM, "m", font(FS), col);
        text_at(p, m.right() + 1.0, base, Align2::LEFT_BOTTOM, "s", font(FS), col);
        text_at(p, disp.right() - 21.0, base, Align2::RIGHT_BOTTOM, &format!("{ms:.2}"), super::theme::thin(13.0), col);
    }
    text_at(p, disp.right() - 1.0, t + 89.0, Align2::RIGHT_BOTTOM, &format!("{:.2} kHz SR", sr / 1000.0), font(FS), dimc);

    // Delay Mode.
    text_at(p, l + 7.0, t + 108.0, Align2::LEFT_BOTTOM, "Delay Mode", font(FS), text());
    let dd = Rect::from_min_size(pos2(l + 7.0, t + 112.0), vec2(98.0, 13.0));
    if let Some(i) = chooser(cx, ui, p, dd, ap::MODE, a0.mode.label(), &DelayMode::ALL.map(|m| m.label()), true)
        && let FxParams::AlignDelay(a) = &mut cx.params
    {
        a.mode = DelayMode::ALL[i];
    }

    // L / R values with the link toggle.
    text_at(p, l + 7.0, t + 141.0, Align2::LEFT_BOTTOM, "L", font(FS), text());
    text_at(p, l + 7.0, t + 157.0, Align2::LEFT_BOTTOM, "R", font(FS), text());
    let lk = Rect::from_min_size(pos2(l + 16.0, t + 140.0), vec2(19.0, 13.0));
    boxed(p, lk, gray(0x1e));
    let gc = if a0.link { ORANGE } else { text() };
    let s = Stroke::new(1.0, gc);
    let (x, y) = (lk.left() + 1.0, lk.top());
    p.rect_stroke(Rect::from_min_max(pos2(x + 5.5, y + 4.5), pos2(x + 11.5, y + 8.5)), CornerRadius::ZERO, s, egui::StrokeKind::Middle);
    p.line_segment([pos2(x + 3.0, y + 6.5), pos2(x + 7.0, y + 6.5)], s);
    p.line_segment([pos2(x + 10.0, y + 6.5), pos2(x + 14.0, y + 6.5)], s);
    if a0.link {
        p.line_segment([pos2(x + 7.0, y + 6.5), pos2(x + 10.0, y + 6.5)], s);
    }
    if cx.toggle(ui, lk, ap::LINK, true)
        && let FxParams::AlignDelay(a) = &mut cx.params
    {
        a.link = !a.link;
    }
    let dist = a0.mode == DelayMode::Distance;
    let shown = |a: &AlignDelayParams, right: bool| {
        let v = align_value(a, right);
        match a.mode {
            DelayMode::Samples => format!("{v:.0}"),
            DelayMode::Distance if a.feet => format!("{:.2}", v * FEET_PER_M),
            _ => format!("{v:.2}"),
        }
    };
    for (k, top) in [(ap::LEFT, t + 131.0), (ap::RIGHT, t + 147.0)] {
        let r = Rect::from_min_size(pos2(l + 38.0, top), vec2(47.0, 13.0));
        let enabled = !(k == ap::RIGHT && a0.link);
        value_box(cx, ui, p, r, k, &shown(&a0, k == ap::RIGHT), enabled);
        if !dist {
            let unit = if a0.mode == DelayMode::Samples { "smp" } else { "ms" };
            text_at(p, l + 89.0, top + 10.0, Align2::LEFT_BOTTOM, unit, font(FS), text());
        }
    }
    if dist {
        let ub = Rect::from_min_size(pos2(l + 89.0, t + 138.0), vec2(19.0, 13.0));
        toggle_look(p, ub, if a0.feet { "ft" } else { "m" }, false, true);
        if cx.toggle(ui, ub, ap::FEET, true)
            && let FxParams::AlignDelay(a) = &mut cx.params
        {
            a.feet = !a.feet;
        }
        let tb = Rect::from_min_size(pos2(l + 38.0, t + 168.0), vec2(47.0, 13.0));
        let temp = if a0.fahrenheit { a0.temp * 9.0 / 5.0 + 32.0 } else { a0.temp };
        value_box(cx, ui, p, tb, ap::TEMP, &format!("{temp:.2}"), true);
        let cb = Rect::from_min_size(pos2(l + 89.0, t + 168.0), vec2(19.0, 13.0));
        toggle_look(p, cb, if a0.fahrenheit { "°F" } else { "°C" }, false, true);
        if cx.toggle(ui, cb, ap::FAHRENHEIT, true)
            && let FxParams::AlignDelay(a) = &mut cx.params
        {
            a.fahrenheit = !a.fahrenheit;
        }
    }
}

/// Max number box: #242424, teal fill for the value, #919191 outline when
/// focused; drag vertically, double-click to type.
fn value_box(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, shown: &str, enabled: bool) {
    let eid = egui::Id::unique(("devp_edit", cx.dev.id, k));
    let editing = ui.data(|d| d.get_temp::<String>(eid));
    boxed(p, r, gray(0x24));
    let inner = r.shrink(1.0);
    if let Some(v) = norm(&cx.params, k) {
        let w = (v.clamp(0.0, 1.0) * inner.width()).round();
        if w > 0.0 {
            let fill = if enabled { BOX_TEAL } else { mix(BOX_TEAL, gray(0x24), 0.6) };
            p.rect_filled(Rect::from_min_size(inner.min, vec2(w, inner.height())), CornerRadius::ZERO, fill);
        }
    }
    if cx.focused(k) {
        p.rect_stroke(r.expand(1.0), CornerRadius::ZERO, Stroke::new(1.0, gray(0x91)), egui::StrokeKind::Inside);
    }
    let resp = cx.drag(ui, r, k, enabled);
    if let Some(mut s) = editing {
        let te = egui::TextEdit::singleline(&mut s)
            .font(font(FS))
            .align(egui::Align2::CENTER_CENTER)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .text_color(text());
        let out = ui.put(inner, te);
        out.request_focus();
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if enter || esc || out.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(eid));
            if !esc && let Ok(x) = s.trim().parse::<f32>() {
                cx.undo_step();
                set_typed(&mut cx.params, k, x);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(eid, s));
        }
        return;
    }
    let col = if enabled { text() } else { dim() };
    text_at(p, r.center().x, r.top() + 10.0, Align2::CENTER_BOTTOM, shown, font(FS), col);
    if enabled && resp.double_clicked() {
        ui.data_mut(|d| d.insert_temp(eid, shown.to_string()));
    }
}

/// A typed value in display units.
fn set_typed(p: &mut FxParams, k: u8, x: f32) {
    if !x.is_finite() {
        return;
    }
    if let FxParams::AlignDelay(a) = p {
        match k {
            ap::LEFT | ap::RIGHT => {
                let x = if a.mode == DelayMode::Distance && a.feet { x / FEET_PER_M } else { x };
                set_align_value(a, k == ap::RIGHT, x);
            }
            ap::TEMP => {
                let c = if a.fahrenheit { (x - 32.0) * 5.0 / 9.0 } else { x };
                a.temp = c.clamp(ALIGN_TEMP_MIN_C, ALIGN_TEMP_MAX_C);
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------------------
// Drops from the browser / track headers.
// ----------------------------------------------------------------------------

/// Effect dropped on a track (header, lane or slot): appended to its chain.
pub fn effect_on_track(app: &mut DawApp, t: usize, fx: FxParams) {
    let Some(id) = app.project.tracks.get(t).map(|x| x.id) else { return };
    app.insert_device(ChainRef::Track(id), Some(usize::MAX), fx);
    app.clear_structure_selection();
    app.selected_track = t;
    app.master_selected = false;
    app.selected_return = None;
}

/// Effect dropped into empty space: a new audio track holding it (Live).
pub fn effect_on_new_track(app: &mut DawApp, fx: FxParams) {
    let t = app.edit(|p| p.add_track(daw_project::TrackKind::Audio));
    effect_on_track(app, t, fx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_taper_round_trips() {
        for db in [-60.0, -30.0, -18.0, -6.0, 0.0, 12.0, 35.0] {
            assert!((norm_to_gain(gain_to_norm(db)) - db).abs() < 1e-3, "{db}");
        }
        assert_eq!(gain_to_norm(0.0), 0.5);
        assert_eq!(norm_to_gain(1.0), 35.0);
        assert_eq!(norm_to_gain(0.0), UTIL_GAIN_MIN_DB);
        assert_eq!(fmt_db(UTIL_GAIN_MIN_DB), "-inf dB");
        assert_eq!(fmt_db(35.0), "35.0 dB");
        assert_eq!(fmt_db(0.0), "0.00 dB");
    }

    #[test]
    fn gain_arrow_steps() {
        let mut p = FxParams::Utility(UtilityParams { gain_db: -17.0, ..Default::default() });
        step(&mut p, up::GAIN, -1.0);
        let FxParams::Utility(u) = p else { panic!() };
        assert_eq!(u.gain_db, -18.0);
        step(&mut p, up::GAIN, -1.0);
        let FxParams::Utility(u) = p else { panic!() };
        assert!(u.gain_db < -18.5);
        step(&mut p, up::GAIN, 1.0);
        let FxParams::Utility(u) = p else { panic!() };
        assert_eq!(u.gain_db, -18.0);
    }

    #[test]
    fn align_box_fill_matches_live() {
        // Measured in Live: 3.28 ms fills 18 %, 7.60 m 27.5 %, 20 °C 70 %.
        let a = AlignDelayParams { time_l: 3.28, ..Default::default() };
        assert!((norm(&FxParams::AlignDelay(a), ap::LEFT).unwrap() - 0.181).abs() < 0.005);
        let a = AlignDelayParams { mode: DelayMode::Distance, dist_r: 7.6, ..Default::default() };
        assert!((norm(&FxParams::AlignDelay(a), ap::RIGHT).unwrap() - 0.2757).abs() < 0.005);
        assert!((norm(&FxParams::AlignDelay(a), ap::TEMP).unwrap() - 0.7).abs() < 1e-4);
    }

    #[test]
    fn width_and_ms_knob_maps() {
        let mut p = FxParams::Utility(UtilityParams::default());
        assert_eq!(norm(&p, up::WIDTH), Some(0.5));
        set_norm(&mut p, up::WIDTH, 1.0);
        assert!(matches!(p, FxParams::Utility(u) if (u.width - 4.0).abs() < 1e-6));
        reset(&mut p, up::WIDTH);
        assert!(matches!(p, FxParams::Utility(u) if u.width == 1.0));
        let mut p = FxParams::Utility(UtilityParams { ms_mode: true, ..Default::default() });
        set_norm(&mut p, up::WIDTH, 0.2);
        assert!(matches!(p, FxParams::Utility(u) if (u.mid_side + 0.6).abs() < 1e-6));
    }
}
