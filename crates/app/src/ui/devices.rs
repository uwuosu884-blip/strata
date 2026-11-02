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

mod autoshift;
mod chaneq;
mod comp;
mod delay;
mod drumbuss;
mod dynamics;
mod echo;
mod eq8;
mod erosion;
mod fx4;
mod graindelay;
mod lfo;
mod modfx;
mod rack;
mod redux;
mod resonators;
mod reverb;
mod saturator;
mod shaper;
mod spec;
mod specres;
mod spectime;
pub use eq8::{DOTS as EQ_DOTS, arrow as eq_arrow, maximised as eq_maximised, unfolded as eq_unfolded};

const TITLE_H: f32 = 17.0;
/// Space between devices: 2 px, grip (2 + 1 + 2), 2 px.
/// Device chain background grey.
pub const CHAIN_BG: u8 = 0x2a;
pub const GAP: f32 = 9.0;
const TEAL: Color32 = Color32::from_rgb(3, 195, 213);
/// Selected device title bar and its text.
const SEL_TITLE: Color32 = Color32::from_rgb(0xb0, 0xdd, 0xeb);
const SEL_TEXT: Color32 = Color32::from_rgb(0x07, 0x07, 0x07);
/// Value boxes / choosers of a deactivated device lose their dark fill.
fn box_fill(on: bool) -> Color32 {
    if on { gray(0x24) } else { panel() }
}
const BOX_TEAL: Color32 = VOL_TEAL;
/// Knob drag: pixels for the full range.
const DRAG_PX: f32 = 200.0;

/// Width of a folded device (title strip only).
pub const FOLD_W: f32 = 21.0;

pub fn width(d: &Device) -> f32 {
    if d.folded {
        return FOLD_W;
    }
    match d.params {
        FxParams::Utility(_) => 139.0,
        FxParams::AlignDelay(_) => 112.0,
        FxParams::EqEight(_) => eq8::WIDTH,
        FxParams::Rack(_) => rack::width(d),
        FxParams::Gate(g) => dynamics::gate_width(&g),
        FxParams::Glue(g) => dynamics::glue_width(&g),
        FxParams::Limiter(_) => dynamics::LIMITER_W,
        FxParams::Lfo(l) if l.shaper => shaper::WIDTH,
        FxParams::Lfo(_) => lfo::WIDTH,
        FxParams::EqThree(_) => modfx::EQ3_W,
        FxParams::Chorus(_) => modfx::CHO_W,
        FxParams::AutoPan(_) => modfx::PAN_W,
        FxParams::AutoFilter(a) => modfx::af_width(&a),
        FxParams::Compressor(c) => comp::width(&c),
        FxParams::Spectrum(s) => spec::width(&s),
        FxParams::AutoShift(a) => autoshift::width(&a),
        FxParams::ChannelEq(_) => chaneq::WIDTH,
        FxParams::Echo(_) => echo::WIDTH,
        FxParams::Erosion(_) => erosion::WIDTH,
        FxParams::Reverb(_) => reverb::WIDTH,
        FxParams::Redux(_) => redux::WIDTH,
        FxParams::Resonators(_) => resonators::WIDTH,
        FxParams::DrumBuss(_) => drumbuss::WIDTH,
        FxParams::SpectralResonator(_) => specres::WIDTH,
        FxParams::SpectralTime(_) => spectime::WIDTH,
        FxParams::Saturator(s) => {
            if s.expanded {
                saturator::WIDTH_EXT
            } else {
                saturator::WIDTH
            }
        }
        FxParams::Delay(_) => delay::WIDTH,
        FxParams::GrainDelay(_) => graindelay::WIDTH,
    }
}

// ----------------------------------------------------------------------------
// Parameters: normalised values, defaults, display, steps.
// ----------------------------------------------------------------------------

use daw_project::devices::params::fp;
pub use daw_project::devices::params::{P18, align_value, ap, gain_to_norm, mappable, norm, norm_to_gain, set_align_value, set_norm, up};

fn fmt_db(db: f32) -> String {
    if db <= UTIL_GAIN_MIN_DB {
        "-inf dB".into()
    } else if db.abs() < 9.995 {
        format!("{db:.2} dB")
    } else {
        format!("{db:.1} dB")
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
        FxParams::EqEight(e) => eq8::reset(e, k),
        FxParams::Rack(r) => match k {
            0..16 => r.macros[k as usize] = 0.0,
            rack::SELECTOR => r.selector = 0.0,
            _ => {}
        },
        FxParams::Gate(_) | FxParams::Glue(_) | FxParams::Limiter(_) => dynamics::reset(p, k),
        FxParams::Lfo(_) => lfo::reset(p, k),
        FxParams::EqThree(_) | FxParams::Chorus(_) | FxParams::AutoPan(_) | FxParams::AutoFilter(_) => modfx::reset(p, k),
        FxParams::Compressor(_) => comp::reset(p, k),
        FxParams::Spectrum(_) => spec::reset(p, k),
        FxParams::AutoShift(_) => autoshift::reset(p, k),
        FxParams::ChannelEq(_) => chaneq::reset(p, k),
        FxParams::Echo(_) => echo::reset(p, k),
        FxParams::Erosion(_) => erosion::reset(p, k),
        FxParams::Reverb(_) => reverb::reset(p, k),
        FxParams::Redux(_) => redux::reset(p, k),
        FxParams::Resonators(_) => resonators::reset(p, k),
        FxParams::DrumBuss(_) => drumbuss::reset(p, k),
        FxParams::SpectralResonator(_) => specres::reset(p, k),
        FxParams::SpectralTime(_) => spectime::reset(p, k),
        FxParams::Saturator(_) => saturator::reset(p, k),
        FxParams::Delay(_) => delay::reset(p, k),
        FxParams::GrainDelay(_) => graindelay::reset(p, k),
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
        FxParams::EqEight(e) => eq8::step(e, k, dir),
        FxParams::Rack(r) => match k {
            0..16 => r.macros[k as usize] = (r.macros[k as usize].round() + dir).clamp(0.0, 127.0),
            rack::SELECTOR => r.selector = (r.selector.round() + dir).clamp(0.0, 127.0),
            _ => {}
        },
        FxParams::Gate(_) | FxParams::Glue(_) | FxParams::Limiter(_) => dynamics::step(p, k, dir),
        FxParams::Lfo(_) => lfo::step(p, k, dir),
        FxParams::EqThree(_) | FxParams::Chorus(_) | FxParams::AutoPan(_) | FxParams::AutoFilter(_) => modfx::step(p, k, dir),
        FxParams::Compressor(_) => comp::step(p, k, dir),
        FxParams::Spectrum(_) => spec::step(p, k, dir),
        FxParams::AutoShift(_) => autoshift::step(p, k, dir),
        FxParams::ChannelEq(_) => chaneq::step(p, k, dir),
        FxParams::Echo(_) => echo::step(p, k, dir),
        FxParams::Erosion(_) => erosion::step(p, k, dir),
        FxParams::Reverb(_) => reverb::step(p, k, dir),
        FxParams::Redux(_) => redux::step(p, k, dir),
        FxParams::Resonators(_) => resonators::step(p, k, dir),
        FxParams::DrumBuss(_) => drumbuss::step(p, k, dir),
        FxParams::SpectralResonator(_) => specres::step(p, k, dir),
        FxParams::SpectralTime(_) => spectime::step(p, k, dir),
        FxParams::Saturator(_) => saturator::step(p, k, dir),
        FxParams::Delay(_) => delay::step(p, k, dir),
        FxParams::GrainDelay(_) => graindelay::step(p, k, dir),
    }
}

const MACRO_NAMES: [&str; 16] = [
    "Macro 1", "Macro 2", "Macro 3", "Macro 4", "Macro 5", "Macro 6", "Macro 7", "Macro 8", "Macro 9", "Macro 10", "Macro 11", "Macro 12",
    "Macro 13", "Macro 14", "Macro 15", "Macro 16",
];

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
        FxParams::EqEight(_) => eq8::name(k),
        FxParams::Rack(_) => MACRO_NAMES.get(k as usize).copied().unwrap_or("Chain Selector"),
        FxParams::Gate(_) | FxParams::Glue(_) | FxParams::Limiter(_) => dynamics::name(p, k),
        FxParams::Lfo(_) => lfo::name(k),
        FxParams::EqThree(_) | FxParams::Chorus(_) | FxParams::AutoPan(_) | FxParams::AutoFilter(_) => modfx::name(p, k),
        FxParams::Compressor(_) => comp::name(k),
        FxParams::Spectrum(_) => spec::name(k),
        FxParams::AutoShift(_) => autoshift::name(k),
        FxParams::ChannelEq(_) => chaneq::name(k),
        FxParams::Echo(_) => echo::name(k),
        FxParams::Erosion(_) => erosion::name(k),
        FxParams::Reverb(_) => reverb::name(k),
        FxParams::Redux(_) => redux::name(k),
        FxParams::Resonators(_) => resonators::name(k),
        FxParams::DrumBuss(_) => drumbuss::name(k),
        FxParams::SpectralResonator(_) => specres::name(k),
        FxParams::SpectralTime(_) => spectime::name(k),
        FxParams::Saturator(_) => saturator::name(k),
        FxParams::Delay(_) => delay::name(k),
        FxParams::GrainDelay(_) => graindelay::name(k),
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

/// Live text toggle (13 px tall): #1e off, orange on, greyed when disabled;
/// a deactivated device keeps the fill and greys the label.
fn toggle_look(p: &Painter, r: Rect, label: &str, on: bool, enabled: bool, dev_on: bool) {
    let (fill, col) = match (on, enabled) {
        _ if !dev_on => (if on { mix(ORANGE, gray(0x36), 0.5) } else { gray(0x1e) }, gray(0x69)),
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
    knob_look_m(p, c, v, def, enabled, true);
}

/// `knob_look`; `marker` = the triangle above (bipolar / Utility knobs).
fn knob_look_m(p: &Painter, c: Pos2, v: f32, def: f32, enabled: bool, marker: bool) {
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
    if marker {
        p.add(Shape::convex_polygon(
            vec![pos2(c.x - 3.5, c.y - 18.5), pos2(c.x + 3.5, c.y - 18.5), pos2(c.x, c.y - 14.0)],
            tri,
            Stroke::NONE,
        ));
    }
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

/// Device meter fill colours (Live 12): RMS, peak, held peak, over 0 dB.
const M_RMS: Color32 = Color32::from_rgb(0x00, 0xf7, 0x58);
const M_PEAK: Color32 = Color32::from_rgb(0x08, 0xab, 0x42);
const M_HOLD: Color32 = Color32::from_rgb(0x04, 0xf8, 0x00);
const M_OVER: Color32 = Color32::from_rgb(0xff, 0x4a, 0x3a);

/// Stereo level meter between devices (and at the chain start): two 2 px
/// bars 1 px apart, inset 40 px, the 0 dB notch 11 px below the top. Peak
/// (dim), RMS (bright) and a 1 px held peak on Live's meter scale.
pub fn meter(p: &Painter, x: f32, top: f32, bottom: f32, m: Option<&crate::app::DevMeter>) {
    let (gy0, gy1) = (top + 40.0, bottom - 40.0);
    if gy1 <= gy0 + 12.0 {
        return;
    }
    let h = gy1 - gy0;
    let y_of = |a: f32| -> Option<f32> { (a > 1e-4).then(|| gy0 + super::widgets::meter_frac(20.0 * a.log10()) * h) };
    let notch = gy0 + 11.0;
    for (c, dx) in [0.0f32, 3.0].into_iter().enumerate() {
        let col = |y0: f32, y1: f32| Rect::from_min_max(pos2(x + dx, y0), pos2(x + dx + 2.0, y1));
        p.rect_filled(col(gy0, gy1), 0.0, gray(0x18));
        if let Some(m) = m {
            // Sub-pixel edges like Live's (anti-aliased by the mesh).
            for (a, fill) in [(m.peak[c], M_PEAK), (m.rms[c], M_RMS)] {
                if let Some(y) = y_of(a) {
                    quad(p, col(y, gy1), fill);
                    if y < notch {
                        quad(p, col(y, notch), M_OVER);
                    }
                }
            }
            if let Some(y) = y_of(m.hold[c]).filter(|_| m.hold[c] > m.rms[c]) {
                quad(p, col(y - 0.5, y + 0.5), if y < notch { M_OVER } else { M_HOLD });
            }
        }
        p.rect_filled(col(notch, notch + 1.0), 0.0, gray(0x36));
    }
}

/// MIDI track chain start: a dotted 3 px column (Live's MIDI meter, unlit).
pub fn midi_dots(p: &Painter, x: f32, top: f32, bottom: f32) {
    // 3×3 dots every 5 px (Live's are soft-edged: #18 core, ~#1C average).
    let mut y = top + 45.0;
    while y + 3.0 <= bottom - 40.0 {
        p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(3.0, 3.0)), 0.0, gray(0x1c));
        y += 5.0;
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
    /// Map mode of an enclosing rack: (rack chain, rack id, armed macro).
    map: Option<(ChainRef, u64, Option<u8>)>,
    /// Parameters driven by a macro (shown, not editable).
    mapped: Vec<u8>,
    /// LFO modulation now: (param, modulated value 0..1, Remote mode).
    mods: Vec<(u8, f32, bool)>,
    /// Extra widget-id salt for several widgets on one parameter.
    salt: u32,
}

impl<'a> Ctx<'a> {
    fn new(app: &'a mut DawApp, chain: ChainRef, dev: Device) -> Self {
        let map = rack::map_ctx(app, chain);
        let mapped = if matches!(chain, ChainRef::Sub(_)) {
            (0..=255u8).filter(|&k| norm(&dev.params, k).is_some() && app.mapped(chain, dev.id, k).is_some()).collect()
        } else {
            Vec::new()
        };
        let mut mods: Vec<(u8, f32, bool)> = Vec::new();
        for (k, m, out) in app.lfo_maps_on(dev.id) {
            if !mappable(&dev.params, k) {
                continue;
            }
            let base = mods.iter().rev().find(|x| x.0 == k).map(|x| x.1).or_else(|| norm(&dev.params, k)).unwrap_or(0.0);
            mods.push((k, m.apply(base, out), m.remote));
        }
        if !mods.is_empty() {
            app.animating = true;
        }
        Self { app, chain, params: dev.params, on: dev.on, dev, gesture: false, map, mapped, mods, salt: 0 }
    }

    /// Modulated value of `k` (and whether an LFO drives it in Remote mode).
    fn modv(&self, k: u8) -> Option<(f32, bool)> {
        let last = self.mods.iter().rev().find(|m| m.0 == k)?;
        Some((last.1, self.remote(k)))
    }

    fn remote(&self, k: u8) -> bool {
        self.mods.iter().any(|m| m.0 == k && m.2)
    }

    fn is_mapped(&self, k: u8) -> bool {
        self.mapped.contains(&k)
    }

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
        let id = egui::Id::unique(("devp", self.dev.id, k, self.salt));
        // LFO Map armed: a click on a parameter of another device maps it.
        if let Some((lfo, slot)) = self.app.lfo_arm
            && lfo != self.dev.id
            && mappable(&self.params, k)
        {
            ui.painter().rect_filled(r, CornerRadius::ZERO, rack::MAP_FILL);
            let resp = ui.interact(r, id.with("lfomap"), Sense::click());
            if resp.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(lfo_req_id(), (lfo, slot, self.dev.id, k)));
            }
            return ui.interact(r, id, Sense::hover());
        }
        // Map mode: mappable parameters get an overlay; a click maps the
        // parameter to the armed macro instead of editing it.
        if let Some((_, rack, armed)) = self.map
            && norm(&self.params, k).is_some()
        {
            let hit = self.is_mapped(k);
            ui.painter().rect_filled(r, CornerRadius::ZERO, if hit { rack::MAP_HIT } else { rack::MAP_FILL });
            let resp = ui.interact(r, id.with("map"), Sense::click());
            if resp.clicked()
                && let Some(m) = armed
            {
                rack::request_map(ui.ctx(), rack, m, self.dev.id, k);
            }
            return ui.interact(r, id, Sense::hover());
        }
        ui.interact(r, id, s)
    }

    /// Click toggle: focus + flip.
    fn toggle(&mut self, ui: &egui::Ui, r: Rect, k: u8, enabled: bool) -> bool {
        let enabled = enabled && !self.is_mapped(k) && !self.remote(k);
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

    /// Drag on a continuous parameter like the track header knobs (cursor
    /// hidden and parked, up / right raises); double-click = default.
    fn drag(&mut self, ui: &egui::Ui, r: Rect, k: u8, enabled: bool) -> Response {
        let enabled = enabled && !self.is_mapped(k) && !self.remote(k);
        let resp = self.sense(ui, r, k, Sense::click_and_drag());
        if resp.drag_started() || resp.clicked() {
            self.focus(k);
        }
        let d = if enabled { super::widgets::endless_drag(ui, &resp) } else { egui::Vec2::ZERO };
        // Unsnapped drag position: stepped parameters (synced rates, Glue
        // times...) would otherwise round slow movements back every frame.
        let acc_id = resp.id.with("drag_acc");
        if resp.drag_started() {
            let v0 = norm(&self.params, k).unwrap_or(0.0);
            ui.data_mut(|m| m.insert_temp(acc_id, v0));
        }
        if enabled && resp.double_clicked() {
            self.undo_step();
            reset(&mut self.params, k);
        } else if d != egui::Vec2::ZERO {
            let dv = d.y - d.x;
            if dv != 0.0 {
                self.undo_step();
                let fine = ui.input(|i| i.modifiers.shift);
                let cur = norm(&self.params, k).unwrap_or(0.0);
                let acc = ui.data(|m| m.get_temp::<f32>(acc_id)).unwrap_or(cur);
                let v = (acc - dv / if fine { DRAG_PX * 10.0 } else { DRAG_PX }).clamp(0.0, 1.0);
                ui.data_mut(|m| m.insert_temp(acc_id, v));
                set_norm(&mut self.params, k, v);
            }
        }
        if resp.drag_stopped() {
            ui.data_mut(|m| m.remove::<f32>(acc_id));
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
        let rack_macro = match self.params {
            FxParams::Rack(q) if k < 16 => Some((q.no_rand & (1 << k) != 0, q.no_var & (1 << k) != 0)),
            _ => None,
        };
        let (mut ex_rand, mut ex_var) = (false, false);
        let mapped = self.is_mapped(k);
        let free = if !mapped && norm(&self.params, k).is_some() { rack::free_macro(self.app, self.chain) } else { None };
        let (mut map_to, mut del_map, mut inv_map) = (false, false, false);
        menu::context(resp, 223.0, |ui| {
            if let Some((r, v)) = rack_macro {
                ex_rand = menu::check_item(ui, "Exclude Macro from Randomization", "", r, true);
                ex_var = menu::check_item(ui, "Exclude Macro From Variations", "", v, true);
                menu::sep(ui);
            }
            if let Some((_, _, m)) = free {
                map_to = menu::item(ui, &format!("Map to Macro {}", m + 1), "", true);
                menu::sep(ui);
            }
            if mapped {
                del_map = menu::item(ui, "Delete Mapping", "", true);
                inv_map = menu::item(ui, "Invert Range", "", true);
                menu::sep(ui);
            }
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
        if (ex_rand || ex_var) && matches!(self.params, FxParams::Rack(_)) {
            self.undo_step();
        }
        if let FxParams::Rack(q) = &mut self.params {
            if ex_rand {
                q.no_rand ^= 1 << k;
            }
            if ex_var {
                q.no_var ^= 1 << k;
            }
        }
        if let Some((rc, rid, m)) = free.filter(|_| map_to) {
            rack::add_mapping(self.app, rc, rid, m, self.dev.id, k);
        }
        if del_map || inv_map {
            rack::edit_mapping(self.app, self.chain, self.dev.id, k, inv_map);
        }
    }
}

// ----------------------------------------------------------------------------
// Device frame + chain.
// ----------------------------------------------------------------------------

/// Draw device `idx` of chain `c` in `r` (full height).
/// Body / title clicks on device `id`: plain click selects it alone, Ctrl
/// toggles, Shift selects a range; a drag or right-click on an unselected
/// device selects it alone (a selected one keeps the selection).
fn lfo_req_id() -> egui::Id {
    egui::Id::unique("lfo_map_request")
}

/// Apply a pending LFO map click (made on any device this frame or last).
fn take_lfo_request(app: &mut DawApp, ctx: &egui::Context) {
    if let Some((lfo, slot, dev, k)) = ctx.data_mut(|d| d.remove_temp::<(u64, u8, u64, u8)>(lfo_req_id())) {
        app.lfo_set_map(lfo, slot, dev, k);
        app.lfo_arm = None;
    }
}

pub(super) fn select_clicks(app: &mut DawApp, ui: &egui::Ui, c: ChainRef, id: u64, body: &egui::Response, title: &egui::Response) {
    let mods = ui.input(|i| i.modifiers);
    if body.clicked() || title.clicked() {
        app.click_device(c, id, mods);
    } else if (body.secondary_clicked() || title.secondary_clicked() || title.drag_started()) && !app.dev_selected(id) {
        app.select_only(Some(id));
    } else if !(body.secondary_clicked() || title.secondary_clicked() || title.drag_started()) {
        return;
    }
    app.dev_focus = None;
    app.focus_detail = true;
}

pub fn device(app: &mut DawApp, ui: &mut egui::Ui, r: Rect, c: ChainRef, idx: usize) {
    take_lfo_request(app, ui.ctx());
    let Some(dev) = chain(&app.project, c).get(idx).cloned() else { return };
    if dev.folded {
        return folded_device(app, ui, r, c, &dev);
    }
    if matches!(dev.params, FxParams::Rack(_)) {
        return rack::rack(app, ui, r, c, idx);
    }
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let (l, t) = (r.left(), r.top());
    // Devices inside a rack sit flush in its frame (square corners).
    let nested = matches!(c, ChainRef::Sub(_));
    let rad = if nested { 0 } else { 3 };
    p.rect_filled(r, CornerRadius::same(rad), panel());
    let title = Rect::from_min_size(r.min, vec2(r.width(), TITLE_H));
    // Selected device (or a selected rack around it): light blue title bar
    // with dark name (Live).
    let selected = app.dev_selected(dev.id) || rack::ancestor_selected(app, c);
    p.rect_filled(title, CornerRadius { nw: rad, ne: rad, sw: 0, se: 0 }, if selected { SEL_TITLE } else { gray(0x46) });
    let mut cx = Ctx::new(app, c, dev.clone());

    // A click anywhere on the device selects it (widgets created later win).
    let body = ui.interact(r, egui::Id::unique(("dev_body", dev.id)), Sense::click());
    // Title strip: select / drag / context menu.
    let tr = ui.interact(title, egui::Id::unique(("dev_title", dev.id)), Sense::click_and_drag());
    select_clicks(cx.app, ui, c, dev.id, &body, &tr);
    latency_hover(cx.app, &tr, &dev);
    // Double-click on the title bar folds the device (Live).
    let fold_now = tr.double_clicked();
    tr.dnd_set_drag_payload(DragItem::ChainDevice { chain: c, id: dev.id });
    if tr.dragged() {
        super::cursors::set(ui.ctx(), super::cursors::Cur::Drag);
    }
    let mut delete = false;
    let mut dup = false;
    let mut oversample = false;
    let mut group = false;
    let mut sc_mono = false;
    let mut fold_menu = false;
    let af_mono = match dev.params {
        FxParams::AutoFilter(a) => Some(a.sc_mono),
        _ => None,
    };
    // Device-specific context switches: (label, on).
    let extra: Vec<(&str, bool)> = match dev.params {
        FxParams::Saturator(s) => vec![("Hi-Quality", s.hq), ("Pre-DC Filter", s.pre_dc)],
        FxParams::Delay(d) => vec![("Hi-Quality", d.hq), ("Equal-Loudness", d.eq_loud)],
        _ => Vec::new(),
    };
    let mut extra_hit: Option<usize> = None;
    let eq_os = match dev.params {
        FxParams::EqEight(e) => Some(e.oversample),
        FxParams::Glue(g) => Some(g.oversample),
        _ => None,
    };
    menu::context(&tr, 223.0, |ui| {
        menu::item(ui, "Cut", "Ctrl+X", false);
        menu::item(ui, "Copy", "Ctrl+C", false);
        dup = menu::item(ui, "Duplicate", "Ctrl+D", true);
        delete = menu::item(ui, "Delete", "Del", true);
        menu::sep(ui);
        menu::item(ui, "Rename", "Ctrl+R", false);
        menu::item(ui, "Edit Info Text", "", false);
        menu::sep(ui);
        group = menu::item(ui, "Group", "Ctrl+G", true);
        group |= menu::item(ui, "Group to Audio Effect Rack", "", true);
        menu::sep(ui);
        fold_menu |= menu::item(ui, "Fold", "", true);
        menu::item(ui, "Show Preset Name", "", false);
        menu::sep(ui);
        menu::item(ui, "Save as Default Preset", "", false);
        menu::item(ui, "Lock to Control Surface", "", false);
        if let Some(on) = eq_os {
            menu::sep(ui);
            oversample = menu::check_item(ui, "Oversampling", "", on, true);
        }
        if let Some(on) = af_mono {
            menu::sep(ui);
            sc_mono = menu::check_item(ui, "Mono Sidechain", "", on, true);
        }
        if !extra.is_empty() {
            menu::sep(ui);
            for (i, (label, on)) in extra.iter().enumerate() {
                if menu::check_item(ui, label, "", *on, true) {
                    extra_hit = Some(i);
                }
            }
        }
    });

    // Activator.
    let ac = pos2(l + 9.5, t + 8.5);
    p.circle_filled(ac, 6.5, gray(0x11));
    p.circle_filled(ac, 5.5, if cx.on { ORANGE } else { gray(0x1e) });
    let ar = ui.interact(Rect::from_center_size(ac, vec2(13.0, 13.0)), egui::Id::unique(("dev_on", dev.id)), Sense::click());
    if ar.clicked() {
        cx.undo_step();
        cx.on = !cx.on;
    }
    // Header buttons (hot-swap, save, menu): not implemented, greyed.
    let right = r.right();
    let glyph = gray(0x75);
    for (i, dx) in [42.5, 26.5, 10.5].into_iter().enumerate() {
        rack::header_button(&p, pos2(right - dx, t + 8.5), i);
    }
    let mut name_right = right - 49.0 - 2.0;
    if matches!(dev.params, FxParams::Glue(_)) {
        dynamics::cytomic(&p, right, t);
    }
    if matches!(dev.params, FxParams::AlignDelay(_) | FxParams::Lfo(_)) {
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
    let mut name_x = l + 20.0;
    if matches!(dev.params, FxParams::EqEight(_) | FxParams::Spectrum(_)) {
        eq8::header_button(&mut cx, ui, &p, l, t);
        name_x = l + 36.0;
    }
    if dev.params.sidechain().is_some_and(|s| s.shown) && !matches!(dev.params, FxParams::AutoFilter(_)) {
        dynamics::title_fold(&mut cx, ui, &p, l, t);
        name_x = l + 38.0;
    }
    if matches!(dev.params, FxParams::Saturator(_)) {
        saturator::title_button(&mut cx, ui, &p, l, t);
        name_x = l + 38.0;
    }
    let nf = bold(FS);
    let name = fit(&p, dev.params.name(), &nf, name_right - name_x);
    text_at(&p, name_x, t + 12.0, Align2::LEFT_BOTTOM, &name, nf, if selected { SEL_TEXT } else { text() });

    match dev.params {
        FxParams::Utility(_) => utility(&mut cx, ui, &p, l, t),
        FxParams::AlignDelay(_) => align_delay(&mut cx, ui, &p, l, t),
        FxParams::EqEight(_) => eq8::eq_eight(&mut cx, ui, &p, r),
        FxParams::Gate(_) => dynamics::gate(&mut cx, ui, &p, l, t),
        FxParams::Glue(_) => dynamics::glue(&mut cx, ui, &p, l, t),
        FxParams::Limiter(_) => dynamics::limiter(&mut cx, ui, &p, l, t),
        FxParams::Lfo(lp) if lp.shaper => shaper::shaper(&mut cx, ui, &p, l, t),
        FxParams::Lfo(_) => lfo::lfo(&mut cx, ui, &p, l, t),
        FxParams::EqThree(_) => modfx::eq_three(&mut cx, ui, &p, r),
        FxParams::Chorus(_) => modfx::chorus(&mut cx, ui, &p, r),
        FxParams::AutoPan(_) => modfx::auto_pan(&mut cx, ui, &p, r),
        FxParams::AutoFilter(_) => modfx::auto_filter(&mut cx, ui, &p, r),
        FxParams::Compressor(_) => comp::compressor(&mut cx, ui, &p, r),
        FxParams::Spectrum(_) => spec::spectrum(&mut cx, ui, &p, r),
        FxParams::AutoShift(_) => autoshift::auto_shift(&mut cx, ui, &p, r),
        FxParams::ChannelEq(_) => chaneq::channel_eq(&mut cx, ui, &p, r),
        FxParams::Echo(_) => echo::draw(&mut cx, ui, &p, r),
        FxParams::Erosion(_) => erosion::erosion(&mut cx, ui, &p, r),
        FxParams::Reverb(_) => reverb::draw(&mut cx, ui, &p, r),
        FxParams::Redux(_) => redux::draw(&mut cx, ui, &p, r),
        FxParams::Resonators(_) => resonators::draw(&mut cx, ui, &p, r),
        FxParams::DrumBuss(_) => drumbuss::draw(&mut cx, ui, &p, r),
        FxParams::SpectralResonator(_) => specres::draw(&mut cx, ui, &p, r),
        FxParams::SpectralTime(_) => spectime::draw(&mut cx, ui, &p, r),
        FxParams::Saturator(_) => saturator::draw(&mut cx, ui, &p, r),
        FxParams::Delay(_) => delay::draw(&mut cx, ui, &p, r),
        FxParams::GrainDelay(_) => graindelay::draw(&mut cx, ui, &p, r),
        FxParams::Rack(_) => {}
    }
    if sc_mono && let FxParams::AutoFilter(a) = &mut cx.params {
        a.sc_mono = !a.sc_mono;
        cx.undo_step();
    }
    if let Some(i) = extra_hit {
        match &mut cx.params {
            FxParams::Saturator(s) if i == 0 => s.hq = !s.hq,
            FxParams::Saturator(s) => s.pre_dc = !s.pre_dc,
            FxParams::Delay(d) if i == 0 => d.hq = !d.hq,
            FxParams::Delay(d) => d.eq_loud = !d.eq_loud,
            _ => {}
        }
        cx.undo_step();
    }
    if oversample {
        match &mut cx.params {
            FxParams::EqEight(e) => e.oversample = !e.oversample,
            FxParams::Glue(g) => g.oversample = !g.oversample,
            _ => {}
        }
        cx.undo_step();
    }
    let app = cx.commit();
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
    if delete {
        match app.selected_devices() {
            Some((sc, ids)) if in_sel && ids.len() > 1 => app.delete_devices(sc, &ids),
            _ => app.delete_device(c, dev.id),
        }
    }
    if fold_now || fold_menu {
        app.toggle_device_fold(c, dev.id);
    }
}

/// A track header under a dragged device: outline it and, after a short
/// hover, select the track so its chain shows in the device view (drop
/// there to place the device precisely).
pub fn header_drag_hover(app: &mut DawApp, ui: &egui::Ui, resp: &Response, t: usize) {
    let key = egui::Id::unique("dev_drag_hover");
    let dragging =
        resp.dnd_hover_payload::<DragItem>().is_some_and(|d| matches!(*d, DragItem::ChainDevice { .. } | DragItem::AudioEffect(_)));
    if !dragging {
        if ui.data(|d| d.get_temp::<(usize, f64)>(key)).is_some_and(|(k, _)| k == t) {
            ui.data_mut(|d| d.remove::<(usize, f64)>(key));
        }
        return;
    }
    ui.painter().rect_stroke(resp.rect.shrink(1.0), CornerRadius::ZERO, Stroke::new(1.0, focus()), egui::StrokeKind::Inside);
    let now = ui.input(|i| i.time);
    let since = match ui.data(|d| d.get_temp::<(usize, f64)>(key)) {
        Some((k, t0)) if k == t => t0,
        _ => {
            ui.data_mut(|d| d.insert_temp(key, (t, now)));
            now
        }
    };
    if now - since > 0.6 && app.selected_track != t {
        app.select_track_header(t, crate::arr_ops::SelMode::Single);
    }
    ui.ctx().request_repaint();
}

/// A device dropped on track header `t`: moved to the end of its chain
/// (Ctrl: copied).
pub fn device_dropped_on_track(app: &mut DawApp, ui: &egui::Ui, item: &DragItem, t: usize) -> bool {
    let DragItem::ChainDevice { chain: from, id } = item else { return false };
    let copy = ui.input(|i| i.modifiers.command);
    app.device_to_track(*from, *id, t, copy);
    true
}

/// A folded device: a full-height title strip with the activator on top and
/// the name reading bottom to top. Double-click (or Fold in the context
/// menu) unfolds it; it still selects, drags and shows its latency.
fn folded_device(app: &mut DawApp, ui: &mut egui::Ui, r: Rect, c: ChainRef, dev: &Device) {
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let (l, t) = (r.left(), r.top());
    let nested = matches!(c, ChainRef::Sub(_));
    let selected = app.dev_selected(dev.id) || rack::ancestor_selected(app, c);
    p.rect_filled(r, CornerRadius::same(if nested { 0 } else { 3 }), if selected { SEL_TITLE } else { gray(0x46) });
    let tr = ui.interact(r, egui::Id::unique(("dev_title", dev.id)), Sense::click_and_drag());
    select_clicks(app, ui, c, dev.id, &tr, &tr);
    latency_hover(app, &tr, dev);
    tr.dnd_set_drag_payload(DragItem::ChainDevice { chain: c, id: dev.id });
    if tr.dragged() {
        super::cursors::set(ui.ctx(), super::cursors::Cur::Drag);
    }
    let mut unfold = tr.double_clicked();
    let mut delete = false;
    menu::context(&tr, 223.0, |ui| {
        menu::item(ui, "Cut", "Ctrl+X", false);
        menu::item(ui, "Copy", "Ctrl+C", false);
        menu::item(ui, "Duplicate", "Ctrl+D", false);
        delete = menu::item(ui, "Delete", "Del", true);
        menu::sep(ui);
        unfold |= menu::check_item(ui, "Fold", "", true, true);
    });
    // Activator.
    let ac = pos2(l + 10.5, t + 8.5);
    p.circle_filled(ac, 6.5, gray(0x11));
    p.circle_filled(ac, 5.5, if dev.on { ORANGE } else { gray(0x1e) });
    let ar = ui.interact(Rect::from_center_size(ac, vec2(13.0, 13.0)), egui::Id::unique(("dev_on", dev.id)), Sense::click());
    if ar.clicked() {
        app.begin_gesture();
        app.set_device(c, dev.id, !dev.on, dev.params);
    }
    let f = bold(FS);
    let name = fit(&p, dev.params.name(), &f, (r.height() - 26.0).max(10.0));
    let g = p.layout_no_wrap(name, f, if selected { SEL_TEXT } else { text() });
    let base = t + 20.0 + g.size().x;
    p.add(egui::epaint::TextShape::new(pos2(l + 4.5, base), g, text()).with_angle(-PI / 2.0));
    if delete {
        app.delete_device(c, dev.id);
    } else if unfold {
        app.toggle_device_fold(c, dev.id);
    }
}

/// Device latency (samples): its own lookahead; a rack reports its
/// slowest chain of enabled devices.
pub fn dev_latency(d: &Device, sr: f32) -> usize {
    match &d.rack {
        Some(rk) if matches!(d.params, FxParams::Rack(_)) => {
            rk.chains.iter().map(|c| c.devices.iter().filter(|x| x.on).map(|x| dev_latency(x, sr)).sum::<usize>()).max().unwrap_or(0)
        }
        _ => d.params.latency(sr),
    }
}

/// Hovering a device title shows its latency in the status bar (Live).
pub(super) fn latency_hover(app: &mut DawApp, title: &Response, d: &Device) {
    if title.hovered() {
        let sr = app.audio.info.sample_rate as f32;
        let n = dev_latency(d, sr);
        let ms = n as f32 * 1000.0 / sr.max(1.0);
        let t = format!("{ms:.2}");
        let t = t.trim_end_matches('0').trim_end_matches('.');
        app.status_hover = Some(format!("Latency: {n} samples ({t} ms)"));
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
    toggle_look(p, rl, "Ø L", u0.phase_l, true, cx.on);
    toggle_look(p, rr, "Ø R", u0.phase_r, true, cx.on);
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
    toggle_look(p, mono, "Mono", u0.mono, true, cx.on);
    if cx.toggle(ui, mono, up::MONO, true)
        && let FxParams::Utility(u) = &mut cx.params
    {
        u.mono = !u.mono;
    }
    let bass = Rect::from_min_size(pos2(l + 6.0, t + 153.0), vec2(57.0, 13.0));
    toggle_look(p, bass, "Bass Mono", u0.bass_mono, !u0.mono, cx.on);
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
    p.circle_filled(
        hc,
        5.0,
        if aud && cx.on {
            ORANGE
        } else if aud {
            mix(ORANGE, gray(0x36), 0.5)
        } else {
            gray(0x1e)
        },
    );
    let gcol = match (cx.on, aud) {
        (false, _) => gray(0x69),
        (true, true) => Color32::from_rgb(0x20, 0x20, 0x20),
        (true, false) => gray(0xaa),
    };
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
    toggle_look(p, mr, "Mute", u0.mute, true, cx.on);
    toggle_look(p, dr, "DC", u0.dc, true, cx.on);
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
    knob_look(p, pos2(x, base + 21.5), v, def, enabled && cx.on && !cx.remote(k));
    if let Some((m, _)) = cx.modv(k) {
        let a = angle(m);
        p.circle_filled(pos2(x, base + 21.5) + vec2(a.sin(), -a.cos()) * 11.9, 1.9, gray(0xe0));
    }
    text_at(p, x, base + 42.0, Align2::CENTER_BOTTOM, value, font(FS), text());
    if cx.focused(k) {
        brackets(p, area);
    }
}

/// Live chooser (#242424, ▼ on the right); returns the picked index.
#[allow(clippy::too_many_arguments)]
fn chooser(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, cur: &str, items: &[&str], enabled: bool) -> Option<usize> {
    boxed(p, r, box_fill(cx.on));
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
        let col = if ms > 0.0 && cx.on { ORANGE } else { dimc };
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
    let gc = match (cx.on, a0.link) {
        (false, _) => gray(0x69),
        (true, true) => ORANGE,
        (true, false) => text(),
    };
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
        toggle_look(p, ub, if a0.feet { "ft" } else { "m" }, false, true, cx.on);
        if cx.toggle(ui, ub, ap::FEET, true)
            && let FxParams::AlignDelay(a) = &mut cx.params
        {
            a.feet = !a.feet;
        }
        let tb = Rect::from_min_size(pos2(l + 38.0, t + 168.0), vec2(47.0, 13.0));
        let temp = if a0.fahrenheit { a0.temp * 9.0 / 5.0 + 32.0 } else { a0.temp };
        value_box(cx, ui, p, tb, ap::TEMP, &format!("{temp:.2}"), true);
        let cb = Rect::from_min_size(pos2(l + 89.0, t + 168.0), vec2(19.0, 13.0));
        toggle_look(p, cb, if a0.fahrenheit { "°F" } else { "°C" }, false, true, cx.on);
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
    let bg = box_fill(cx.on);
    value_box_ex(cx, ui, p, r, k, shown, enabled, true, bg);
}

/// `value_box`; an inactive one (EQ Eight band off) is an outline on `bg`
/// without the value fill.
#[allow(clippy::too_many_arguments)]
fn value_box_ex(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, shown: &str, enabled: bool, active: bool, bg: Color32) {
    let eid = egui::Id::unique(("devp_edit", cx.dev.id, k));
    let editing = ui.data(|d| d.get_temp::<String>(eid));
    boxed(p, r, if active { box_fill(cx.on) } else { bg });
    let inner = r.shrink(1.0);
    if let Some(v) = norm(&cx.params, k).filter(|_| active) {
        let w = (v.clamp(0.0, 1.0) * inner.width()).round();
        if w > 0.0 {
            let fill = if !cx.on {
                gray(0x55)
            } else if enabled {
                BOX_TEAL
            } else {
                mix(BOX_TEAL, gray(0x24), 0.6)
            };
            p.rect_filled(Rect::from_min_size(inner.min, vec2(w, inner.height())), CornerRadius::ZERO, fill);
        }
    }
    if cx.focused(k) {
        p.rect_stroke(r.expand(1.0), CornerRadius::ZERO, Stroke::new(1.0, gray(0x91)), egui::StrokeKind::Inside);
    }
    cx.drag(ui, r, k, enabled);
    // Typing on the focused box opens the editor with that character (Live);
    // double-click resets like the knobs.
    let editing = match (editing, cx.focused(k) && enabled) {
        (None, true) => cx.app.dev_typing.take(),
        (e, _) => e,
    };
    if let Some(mut s) = editing {
        let te = egui::TextEdit::singleline(&mut s)
            .font(font(FS))
            .align(egui::Align2::CENTER_CENTER)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .text_color(text());
        let out = ui.put(inner, te);
        if !out.has_focus() {
            out.request_focus();
            // Caret after the typed character, not selecting it.
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), out.id) {
                let end = egui::text::CCursor::new(s.chars().count());
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                st.store(ui.ctx(), out.id);
            }
        }
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
}

/// Parameters edited in a number box (typing opens its editor).
pub fn typable(p: &FxParams, k: u8) -> bool {
    match p {
        FxParams::AlignDelay(a) => k == ap::LEFT || (k == ap::RIGHT && !a.link) || (k == ap::TEMP && a.mode == DelayMode::Distance),
        FxParams::Utility(_) => false,
        FxParams::EqEight(_) => eq8::typable(k),
        FxParams::Rack(_) => false,
        FxParams::Gate(_) | FxParams::Glue(_) | FxParams::Limiter(_) | FxParams::Lfo(_) => false,
        FxParams::EqThree(_) | FxParams::Chorus(_) | FxParams::AutoPan(_) | FxParams::AutoFilter(_) => false,
        FxParams::Compressor(_) | FxParams::Spectrum(_) | FxParams::AutoShift(_) => false,
        FxParams::ChannelEq(_) | FxParams::Echo(_) | FxParams::Erosion(_) | FxParams::Reverb(_) => false,
        FxParams::Redux(_) | FxParams::Saturator(_) | FxParams::Delay(_) | FxParams::GrainDelay(_) => false,
        FxParams::Resonators(_) | FxParams::DrumBuss(_) | FxParams::SpectralResonator(_) | FxParams::SpectralTime(_) => false,
    }
}

/// A typed value in display units.
fn set_typed(p: &mut FxParams, k: u8, x: f32) {
    if !x.is_finite() {
        return;
    }
    if let FxParams::EqEight(e) = p {
        eq8::set_typed(e, k, x);
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
