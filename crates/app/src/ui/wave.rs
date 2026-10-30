//! Waveform drawing. Columns are one pixel wide; each summarises the frames it
//! covers (min / max / RMS) from the 256-frame peak blocks, or from the raw
//! samples when zoomed in. Below ~1.5 frames per pixel the samples are joined
//! by a line. The style is temporarily selectable (Settings > Theme & Colors).

use daw_engine::{PEAK_BLOCK, Peak, SampleBuffer};
use eframe::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WaveStyle {
    /// 1 px vertical min→max line per column (pre-0.7.7 look).
    MinMaxLines,
    /// Min/max envelope as one anti-aliased filled shape.
    #[default]
    MinMaxFilled,
    /// Min/max envelope (light) with the RMS band on top (Audacity).
    PeakRms,
    /// RMS only, mirrored around zero (loudness view).
    Rms,
    /// Absolute peak mirrored around zero (symmetric, SoundCloud-like).
    MirrorPeak,
    /// Min/max on a logarithmic scale (−48 dB … 0 dB): quiet parts visible.
    Decibel,
}

impl WaveStyle {
    pub const ALL: [WaveStyle; 6] = [Self::MinMaxLines, Self::MinMaxFilled, Self::PeakRms, Self::Rms, Self::MirrorPeak, Self::Decibel];
    pub fn label(self) -> &'static str {
        match self {
            Self::MinMaxLines => "1 Min/Max Lines",
            Self::MinMaxFilled => "2 Min/Max Filled",
            Self::PeakRms => "3 Peak + RMS",
            Self::Rms => "4 RMS",
            Self::MirrorPeak => "5 Mirrored Peak",
            Self::Decibel => "6 Min/Max dB",
        }
    }
}

static STYLE: AtomicU8 = AtomicU8::new(1);

/// Set once per frame from the settings.
pub fn set_style(s: WaveStyle) {
    STYLE.store(WaveStyle::ALL.iter().position(|v| *v == s).unwrap_or(1) as u8, Ordering::Relaxed);
}

fn style() -> WaveStyle {
    WaveStyle::ALL[STYLE.load(Ordering::Relaxed) as usize % WaveStyle::ALL.len()]
}

/// What to draw: peak blocks per channel and (if available) the raw samples.
pub struct Source<'a> {
    pub blocks: [&'a [Peak]; 2],
    pub raw: Option<[&'a [f32]; 2]>,
    pub stereo: bool,
    pub len: usize,
}

impl<'a> Source<'a> {
    pub fn of(b: &'a SampleBuffer) -> Self {
        Self { blocks: [b.wave_ch(0), b.wave_ch(1)], raw: Some([&b.left, &b.right]), stereo: b.stereo, len: b.len() }
    }
}

/// Summary of frames `f0..f1` over the channels `chs`.
fn summary(src: &Source, chs: &[usize], f0: f64, f1: f64) -> Option<Peak> {
    let len = src.len as f64;
    if f1 <= 0.0 || f0 >= len || src.len == 0 {
        return None;
    }
    let (f0, f1) = (f0.max(0.0), f1.min(len));
    let mut out: Option<(Peak, f32)> = None;
    let mut add = |p: Peak, n: f32| {
        out = Some(match out {
            Some((o, m)) => (o.merge(p, m, n), m + n),
            None => (p, n),
        })
    };
    let block = PEAK_BLOCK as f64;
    for &ch in chs {
        match src.raw {
            Some(raw) if f1 - f0 < block * 2.0 => {
                let a = (f0 as usize).min(src.len - 1);
                let b = (f1.ceil() as usize).clamp(a + 1, src.len);
                add(Peak::of(&raw[ch][a..b]), (b - a) as f32);
            }
            _ => {
                let pk = src.blocks[ch];
                if pk.is_empty() {
                    continue;
                }
                let i0 = ((f0 / block) as usize).min(pk.len() - 1);
                let i1 = ((f1 / block).ceil() as usize).clamp(i0 + 1, pk.len());
                for p in &pk[i0..i1] {
                    add(*p, block as f32);
                }
            }
        }
    }
    out.map(|o| o.0)
}

/// Linear (or dB) vertical mapping of a gain-scaled value, −1..1 = lane edges.
fn shape_val(v: f32, db: bool) -> f32 {
    if !db {
        return v;
    }
    let a = v.abs();
    if a < 1e-6 {
        return 0.0;
    }
    let n = ((20.0 * a.log10() + 48.0) / 48.0).max(0.0);
    n.copysign(v)
}

/// Draw the waveform of `src` into `body` (columns limited to `visible`).
/// `frame_at_x(x)` = source frame at the left edge of pixel column `x`
/// (None = outside the clip). `gain` scales the drawing only; the painter's
/// clip rect cuts what overflows.
pub fn draw(p: &Painter, body: Rect, visible: Rect, src: &Source, frame_at_x: &dyn Fn(f32) -> Option<f64>, gain: f32, ink: Color32) {
    if src.len == 0 || visible.width() <= 0.0 || body.height() < 1.0 {
        return;
    }
    // Column spans; frames per pixel from the neighbour (kept across loop wraps).
    let x0 = visible.min.x.floor();
    let mut cols: Vec<Option<(f64, f64)>> = Vec::with_capacity(visible.width() as usize + 2);
    let mut fpp = 1.0;
    let mut x = x0;
    while x < visible.max.x {
        let f0 = frame_at_x(x);
        if let (Some(a), Some(b)) = (f0, frame_at_x(x + 1.0))
            && b > a
        {
            fpp = b - a;
        }
        cols.push(f0.map(|a| (a, a + fpp.max(1e-9))));
        x += 1.0;
    }
    let lanes = if src.stereo && body.height() >= 30.0 { 2 } else { 1 };
    let lane_h = body.height() / lanes as f32;
    let zoomed = src.raw.is_some() && fpp < 1.5;
    let st = style();
    for lane in 0..lanes {
        let chs: &[usize] = match (lanes, lane, src.stereo) {
            (2, 0, _) => &[0],
            (2, _, _) => &[1],
            (_, _, true) => &[0, 1],
            _ => &[0],
        };
        let mid = body.min.y + lane_h * (lane as f32 + 0.5);
        let half = lane_h * 0.5;
        // Each lane cuts its own overflow (gain) so L never paints over R.
        let lane_r = Rect::from_x_y_ranges(body.x_range(), (mid - half)..=(mid + half));
        let p = &p.with_clip_rect(lane_r.intersect(p.clip_rect()));
        if zoomed {
            sample_line(p, src, chs[0], &cols, x0, mid, half, gain, ink);
            continue;
        }
        let peaks: Vec<Option<Peak>> = cols.iter().map(|c| c.and_then(|(a, b)| summary(src, chs, a, b))).collect();
        let y = |v: f32, db: bool| mid - shape_val(v * gain, db) * half;
        match st {
            WaveStyle::MinMaxLines => {
                for (i, pk) in peaks.iter().enumerate() {
                    if let Some(pk) = pk {
                        let x = x0 + i as f32;
                        p.line_segment([pos2(x, y(pk.max, false)), pos2(x, y(pk.min, false) + 0.5)], Stroke::new(1.0, ink));
                    }
                }
            }
            WaveStyle::MinMaxFilled => filled(p, &peaks, x0, |pk| (y(pk.max, false), y(pk.min, false)), ink),
            WaveStyle::PeakRms => {
                filled(p, &peaks, x0, |pk| (y(pk.max, false), y(pk.min, false)), ink.gamma_multiply(0.45));
                filled(
                    p,
                    &peaks,
                    x0,
                    |pk| {
                        let r = pk.ms.sqrt();
                        (y(r.min(pk.max.max(0.0)), false), y((-r).max(pk.min.min(0.0)), false))
                    },
                    ink,
                );
            }
            WaveStyle::Rms => filled(
                p,
                &peaks,
                x0,
                |pk| {
                    let r = pk.ms.sqrt();
                    (y(r, false), y(-r, false))
                },
                ink,
            ),
            WaveStyle::MirrorPeak => filled(
                p,
                &peaks,
                x0,
                |pk| {
                    let a = pk.max.max(-pk.min);
                    (y(a, false), y(-a, false))
                },
                ink,
            ),
            WaveStyle::Decibel => filled(p, &peaks, x0, |pk| (y(pk.max, true), y(pk.min, true)), ink),
        }
    }
    if lanes == 2 {
        let yc = body.center().y.round() + 0.5;
        p.line_segment([pos2(visible.min.x, yc), pos2(visible.max.x, yc)], Stroke::new(1.0, ink.gamma_multiply(0.25)));
    }
}

/// One filled band per run of drawable columns: a triangle-strip mesh plus a
/// thin outline (meshes aren't feathered; the outline gives the AA edge).
fn filled(p: &Painter, peaks: &[Option<Peak>], x0: f32, span: impl Fn(&Peak) -> (f32, f32), ink: Color32) {
    let mut i = 0;
    while i < peaks.len() {
        if peaks[i].is_none() {
            i += 1;
            continue;
        }
        let start = i;
        let mut top: Vec<Pos2> = Vec::new();
        let mut bot: Vec<Pos2> = Vec::new();
        while i < peaks.len()
            && let Some(pk) = &peaks[i]
        {
            let (mut t, mut b) = span(pk);
            if b - t < 1.0 {
                // At least one pixel tall (silence stays a visible line).
                let c = 0.5 * (t + b);
                (t, b) = (c - 0.5, c + 0.5);
            }
            let x = x0 + i as f32 + 0.5;
            top.push(pos2(x, t));
            bot.push(pos2(x, b));
            i += 1;
        }
        if i - start == 1 {
            p.line_segment([top[0], bot[0]], Stroke::new(1.0, ink));
            continue;
        }
        let mut mesh = Mesh::default();
        for k in 0..top.len() {
            mesh.colored_vertex(top[k], ink);
            mesh.colored_vertex(bot[k], ink);
            if k > 0 {
                let v = (2 * k) as u32;
                mesh.add_triangle(v - 2, v - 1, v);
                mesh.add_triangle(v - 1, v + 1, v);
            }
        }
        p.add(Shape::mesh(mesh));
        let edge = Stroke::new(0.8, ink);
        p.add(Shape::line(top, edge));
        p.add(Shape::line(bot, edge));
    }
}

/// Zoomed far in: a polyline through the individual samples.
#[allow(clippy::too_many_arguments)]
fn sample_line(p: &Painter, src: &Source, ch: usize, cols: &[Option<(f64, f64)>], x0: f32, mid: f32, half: f32, gain: f32, ink: Color32) {
    let Some(raw) = src.raw else { return };
    let ch = raw[ch];
    let mut pts: Vec<Pos2> = Vec::new();
    let mut last: Option<usize> = None;
    let flush = |pts: &mut Vec<Pos2>| {
        if pts.len() >= 2 {
            p.add(Shape::line(std::mem::take(pts), Stroke::new(1.0, ink)));
        }
        pts.clear();
    };
    for (i, c) in cols.iter().enumerate() {
        let Some((a, b)) = *c else {
            flush(&mut pts);
            last = None;
            continue;
        };
        let first = a.ceil().max(0.0) as usize;
        if last.is_some_and(|l| first < l) {
            flush(&mut pts); // loop wrap: start a new line
        }
        let mut f = first;
        while (f as f64) < b && f < ch.len() {
            if last != Some(f) {
                let x = x0 + i as f32 + ((f as f64 - a) / (b - a)) as f32;
                pts.push(pos2(x, mid - ch[f] * gain * half));
                last = Some(f);
            }
            f += 1;
        }
    }
    flush(&mut pts);
}

/// Helper for callers mapping pixel x → local beat → frame.
pub fn beat_mapper<'a>(x_origin: f32, px_per_beat: f64, frame_of: &'a dyn Fn(f64) -> Option<f64>) -> impl Fn(f32) -> Option<f64> + 'a {
    move |x: f32| frame_of((x - x_origin) as f64 / px_per_beat)
}
