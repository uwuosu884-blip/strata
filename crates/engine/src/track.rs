//! Per-track real-time state: session launch state, clip playback, instrument
//! voices and the channel strip (gain/pan/mute/solo).

use crate::instruments::{Curve, Expr, PadBuf, Voices};
use crate::pv::{Formants, Plans, Pv};
use crate::smoother::Smoothed;
use crate::song::{Clip, ClipContent, NoteMpe, SongTrack};
use crate::warp::{StretchCtx, StretchMode};
use std::f32::consts::{FRAC_PI_4, SQRT_2};

pub(crate) const MAX_EVENTS: usize = 512;
/// Note events other tracks forward to this one per segment.
const MAX_FWD: usize = 256;
/// Hanging notes on a hardware MIDI output (for timed note-offs).
pub(crate) const MAX_OUT_NOTES: usize = 64;
/// Fade length at audio clip edges (samples) — removes start/loop clicks.
const DECLICK: f64 = 64.0;
/// Tolerance for beat comparisons (far below one sample at any tempo).
pub(crate) const EPS: f64 = 1.0e-7;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Action {
    Launch(u32),
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pending {
    pub beat: f64,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoteEvent {
    pub offset: u32,
    pub pitch: u8,
    pub vel: u8,
    pub len: u32,
    /// Expression slot (1-based into `TrackRt::exprs`), 0 = none.
    pub expr: u8,
}

/// Note expressions per segment (clip notes with MPE data).
pub(crate) const MAX_EXPR: usize = 16;

/// A clip note sent from another track. `direct` = straight to the
/// instrument; otherwise it arrives at the input (subject to monitoring).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FwdEvent {
    pub ev: NoteEvent,
    pub direct: bool,
}

/// Timing of the segment being rendered.
pub(crate) struct SegCtx {
    pub beat0: f64,
    pub bps: f64,
    pub n: usize,
    pub playing: bool,
    pub sr: f32,
    pub tempo: f64,
}

pub(crate) struct TrackRt {
    pub in_song: bool,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    gl: Smoothed,
    gr: Smoothed,
    /// (scene, song beat where clip position 0 is)
    pub playing: Option<(u32, f64)>,
    pub pending: Option<Pending>,
    /// A session clip took over this track → arrangement muted until "Back to Arrangement".
    pub session_override: bool,
    pub voices: Voices,
    /// Pre-fader signal of the segment.
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    /// Input signal of the segment (audio input tracks; recorded + monitored).
    pub in_l: Vec<f32>,
    pub in_r: Vec<f32>,
    /// Audio routed into this track by others (summed before the mixer).
    pub bus_l: Vec<f32>,
    pub bus_r: Vec<f32>,
    pub bus_active: bool,
    /// Post-mixer signal (for routing / Post Mixer taps).
    pub post_l: Vec<f32>,
    pub post_r: Vec<f32>,
    /// Individual drum pad outputs.
    pub pads: Vec<PadBuf>,
    pub events: [NoteEvent; MAX_EVENTS],
    pub n_events: usize,
    fwd: [FwdEvent; MAX_FWD],
    n_fwd: usize,
    /// Hardware MIDI out: (pitch, samples left) of sounding clip notes.
    pub out_notes: [(u8, u32); MAX_OUT_NOTES],
    pub n_out_notes: usize,
    pub peak: [f32; 2],
    /// Nothing produced sound in the last segment → mixing can be skipped.
    silent: bool,
    /// Complex / Complex Pro stretcher and its scratch output.
    pv: Box<Pv>,
    tmp_l: Vec<f32>,
    tmp_r: Vec<f32>,
    /// Samples rendered so far (phase-vocoder continuity stamp).
    samples: u64,
    exprs: Box<[Expr; MAX_EXPR]>,
    n_expr: usize,
    /// Session: beat when the playing clip's follow action fires.
    pub follow_at: Option<f64>,
    /// Launch button held (Gate / Repeat): scene.
    pub held: Option<u32>,
    /// Clip envelope modulation of the mixer (volume factor, pan offset).
    pub env_vol: f32,
    pub env_pan: f32,
}

impl TrackRt {
    pub fn new(max_block: usize, sr: f64, plans: &Plans) -> Self {
        Self {
            in_song: false,
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            gl: Smoothed::new(1.0, sr, 0.01),
            gr: Smoothed::new(1.0, sr, 0.01),
            playing: None,
            pending: None,
            session_override: false,
            voices: Voices::default(),
            l: vec![0.0; max_block],
            r: vec![0.0; max_block],
            in_l: vec![0.0; max_block],
            in_r: vec![0.0; max_block],
            bus_l: vec![0.0; max_block],
            bus_r: vec![0.0; max_block],
            bus_active: false,
            post_l: vec![0.0; max_block],
            post_r: vec![0.0; max_block],
            pads: (0..crate::DRUM_PADS).map(|_| PadBuf { l: vec![0.0; max_block], r: vec![0.0; max_block] }).collect(),
            events: [NoteEvent::default(); MAX_EVENTS],
            n_events: 0,
            fwd: [FwdEvent::default(); MAX_FWD],
            n_fwd: 0,
            out_notes: [(0, 0); MAX_OUT_NOTES],
            n_out_notes: 0,
            peak: [0.0; 2],
            silent: true,
            pv: Box::new(Pv::new(plans.clone())),
            tmp_l: vec![0.0; max_block],
            tmp_r: vec![0.0; max_block],
            samples: 0,
            exprs: Box::new([Expr::default(); MAX_EXPR]),
            n_expr: 0,
            follow_at: None,
            held: None,
            env_vol: 1.0,
            env_pan: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.playing = None;
        self.pending = None;
        self.follow_at = None;
        self.held = None;
        self.session_override = false;
        self.voices.kill();
    }

    pub fn apply_mixer(&mut self, st: &SongTrack) {
        self.gain = st.gain;
        self.pan = st.pan;
        self.mute = st.mute;
        self.solo = st.solo;
    }

    /// Queue a note forwarded from another track (dropped when full).
    pub fn push_fwd(&mut self, ev: NoteEvent, direct: bool) {
        if self.n_fwd < MAX_FWD {
            self.fwd[self.n_fwd] = FwdEvent { ev, direct };
            self.n_fwd += 1;
        }
    }

    /// Equal-power pan normalised to unity at centre.
    fn target_gains(&self, audible: bool) -> (f32, f32) {
        if self.mute || !audible {
            return (0.0, 0.0);
        }
        let a = ((self.pan + self.env_pan).clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        let g = self.gain * self.env_vol;
        (g * a.cos() * SQRT_2, g * a.sin() * SQRT_2)
    }

    /// Clip position (beats) of the playing session clip at `beat`.
    pub fn clip_pos(&self, st: &SongTrack, beat: f64) -> Option<f64> {
        let (scene, origin) = self.playing?;
        let clip = st.slots.get(scene as usize)?.as_ref()?;
        let local = (beat - origin).max(0.0);
        Some(clip.content_at(local).unwrap_or(clip.start + clip.length))
    }

    /// Render clips + instrument (pre-fader) into `self.l/r[..n]`.
    /// `clips_on` = false (monitor In) silences clip output while clips keep
    /// running; `midi_monitor` = forwarded "Track In" notes are played.
    /// Returns true if a clip covered part of the segment.
    pub fn render(&mut self, st: &SongTrack, ctx: &SegCtx, clips_on: bool, midi_monitor: bool) -> bool {
        let n = ctx.n;
        let stamp = self.samples;
        let Self { l, r, events, n_events, voices, playing, session_override, fwd, n_fwd, pads, pv, tmp_l, tmp_r, exprs, n_expr, .. } =
            self;
        let (l, r) = (&mut l[..n], &mut r[..n]);
        l.fill(0.0);
        r.fill(0.0);
        *n_events = 0;
        *n_expr = 0;
        let mut produced = false;
        let mut finished = false;
        let mut covered = false;
        let mut out = ClipOut { l, r, events, n_events, pv, tmp_l, tmp_r, exprs, n_expr, stamp };
        // Envelope modulation of the clip that plays at the segment start.
        let mut env: Option<(&Clip, f64)> = None;

        if ctx.playing {
            if let Some((scene, origin)) = *playing {
                match st.slots.get(scene as usize).and_then(Option::as_ref) {
                    Some(clip) => {
                        let span = if clip.looping { f64::INFINITY } else { clip.length };
                        covered |= play_clip(clip, origin, span, ctx, &mut out);
                        finished = !clip.looping && ctx.beat0 + n as f64 * ctx.bps - origin >= clip.length - EPS;
                        env = Some((clip, origin));
                    }
                    None => finished = true, // clip was deleted
                }
            } else if !*session_override {
                let beat1 = ctx.beat0 + n as f64 * ctx.bps;
                for ac in &st.arrangement {
                    if ac.start >= beat1 {
                        break;
                    }
                    if ac.start + ac.clip.length > ctx.beat0 {
                        covered |= play_clip(&ac.clip, ac.start, ac.clip.length, ctx, &mut out);
                        if env.is_none() && ac.start <= ctx.beat0 + EPS {
                            env = Some((&ac.clip, ac.start));
                        }
                    }
                }
            }
        }
        let ClipOut { l, r, events, n_events, exprs, n_expr, .. } = out;
        let exprs: &[Expr; MAX_EXPR] = exprs;
        let _ = n_expr;
        // Clip envelopes → mixer / controllers (block rate).
        let (mut ev, mut ep, mut bend, mut modw) = (1.0, 0.0, 0.0, 0.0);
        if let Some((clip, origin)) = env
            && let Some(c) = clip.content_at(ctx.beat0 - origin)
        {
            let x = &clip.extra;
            // Unlinked envelopes loop over their own length from launch.
            let at = |k: usize| if x.env_loop[k] > 0.0 { (ctx.beat0 - origin).rem_euclid(x.env_loop[k]) } else { c };
            ev = NoteMpe::eval(&x.vol_env, at(0), 1.0);
            ep = NoteMpe::eval(&x.pan_env, at(1), 0.0);
            bend = NoteMpe::eval(&x.bend_env, at(2), 0.0) * 2.0;
            modw = NoteMpe::eval(&x.mod_env, at(3), 0.0);
        }
        voices.set_controls(bend, modw);
        if !clips_on {
            l.fill(0.0);
            r.fill(0.0);
            *n_events = 0;
        }
        produced |= covered && clips_on;
        for f in &fwd[..*n_fwd] {
            if (f.direct || midi_monitor) && *n_events < MAX_EVENTS {
                events[*n_events] = NoteEvent { expr: 0, ..f.ev };
                *n_events += 1;
            }
        }
        *n_fwd = 0;

        // Instrument: render between note events for sample accuracy.
        if st.pad_outs != 0 {
            for p in pads.iter_mut() {
                p.l[..n].fill(0.0);
                p.r[..n].fill(0.0);
            }
        }
        let evs = &mut events[..*n_events];
        evs.sort_unstable_by_key(|e| e.offset); // in-place, never allocates
        let mut pos = 0;
        for e in evs.iter() {
            let off = (e.offset as usize).min(n);
            if off > pos {
                voices.render_split(&st.instrument, &mut l[pos..off], &mut r[pos..off], ctx.sr, st.pad_outs, pads, (pos, off));
                pos = off;
            }
            let ex = (e.expr > 0).then(|| &exprs[e.expr as usize - 1]);
            voices.note_on_expr(&st.instrument, e.pitch, e.vel, e.len, true, ctx.sr, ex);
        }
        if voices.is_active() {
            voices.render_split(&st.instrument, &mut l[pos..], &mut r[pos..], ctx.sr, st.pad_outs, pads, (pos, n));
            produced = true;
        }

        if finished {
            *playing = None;
            voices.release_clip_notes();
        }
        self.silent = !produced;
        self.samples += n as u64;
        self.env_vol = ev;
        self.env_pan = ep;
        covered
    }

    /// Add the input signal (`in_l/in_r`) — monitoring.
    pub fn add_input(&mut self, n: usize) {
        for i in 0..n {
            self.l[i] += self.in_l[i];
            self.r[i] += self.in_r[i];
        }
        self.silent = false;
    }

    /// Add audio routed in from other tracks and clear the bus.
    pub fn add_bus(&mut self, n: usize) {
        if !self.bus_active {
            return;
        }
        for i in 0..n {
            self.l[i] += self.bus_l[i];
            self.r[i] += self.bus_r[i];
        }
        self.bus_l[..n].fill(0.0);
        self.bus_r[..n].fill(0.0);
        self.bus_active = false;
        self.silent = false;
    }

    /// Apply gain/pan (smoothed) into `post_l/post_r`. Returns false when the
    /// result is silent (nothing to route). `audible` is false when another
    /// track is soloed (see `render_segment`).
    pub fn mix(&mut self, n: usize, audible: bool) -> bool {
        let (tl, tr) = self.target_gains(audible);
        self.gl.set_target(tl);
        self.gr.set_target(tr);
        if self.silent {
            // Keep smoothers moving so a later sound starts at the right gain.
            self.gl.skip(n);
            self.gr.skip(n);
            self.post_l[..n].fill(0.0);
            self.post_r[..n].fill(0.0);
            return false;
        }
        let mut pk = [0.0f32; 2];
        for i in 0..n {
            let a = self.l[i] * self.gl.next();
            let b = self.r[i] * self.gr.next();
            self.post_l[i] = a;
            self.post_r[i] = b;
            pk[0] = pk[0].max(a.abs());
            pk[1] = pk[1].max(b.abs());
        }
        self.peak[0] = self.peak[0].max(pk[0]);
        self.peak[1] = self.peak[1].max(pk[1]);
        true
    }
}

/// Where a clip renders to (borrowed pieces of the track state).
struct ClipOut<'a> {
    l: &'a mut [f32],
    r: &'a mut [f32],
    events: &'a mut [NoteEvent; MAX_EVENTS],
    n_events: &'a mut usize,
    pv: &'a mut Pv,
    tmp_l: &'a mut [f32],
    tmp_r: &'a mut [f32],
    exprs: &'a mut [Expr; MAX_EXPR],
    n_expr: &'a mut usize,
    /// Track sample counter at the segment start.
    stamp: u64,
}

#[inline]
fn ramp(x: f64) -> f32 {
    if x >= DECLICK { 1.0 } else { (x / DECLICK).max(0.0) as f32 }
}

/// Play one clip into the buffers (audio) or the event list (MIDI).
/// `origin` = song beat where the clip started playing (start marker),
/// `span` = how long it may play (arrangement length; ∞ for looping session
/// clips). Returns true if the clip covered any sample of this segment.
fn play_clip(clip: &Clip, origin: f64, span: f64, ctx: &SegCtx, out: &mut ClipOut) -> bool {
    let lim = if clip.looping { span } else { span.min(clip.length) };
    let n = ctx.n;
    // First / last sample of the segment inside [origin, origin + lim).
    let first = {
        let d = origin - ctx.beat0;
        if d <= EPS { 0 } else { (((d - EPS) / ctx.bps).ceil() as usize).min(n) }
    };
    let end = {
        let d = origin + lim - ctx.beat0;
        if d <= 0.0 {
            0
        } else if d.is_infinite() {
            n
        } else {
            (((d - EPS) / ctx.bps).ceil() as usize).min(n)
        }
    };
    if first >= end {
        return false;
    }
    match &clip.content {
        ClipContent::Audio(a) => {
            let w = &a.warp;
            let sr_s = a.sample.sample_rate as f64;
            let spb = ctx.bps.recip() / ctx.sr as f64; // seconds per beat
            let loop_len = clip.loop_end - clip.loop_start;
            let fade = w.fade;
            // Distance (samples) to the nearest discontinuity: clip edges and loop wraps.
            // User fades (arrangement), relative to the clip's own edges.
            let user_fade = |local: f64| -> f32 {
                let mut g = 1.0;
                if w.fade_in > 0.0 && local < w.fade_in {
                    g *= crate::song::fade_gain(local / w.fade_in, w.fade_in_curve);
                }
                if w.fade_out > 0.0 && lim.is_finite() && lim - local < w.fade_out {
                    g *= crate::song::fade_gain((lim - local) / w.fade_out, w.fade_out_curve);
                }
                g
            };
            let edge_gain = |local: f64| -> f32 {
                let uf = user_fade(local);
                if !fade {
                    return uf;
                }
                let mut d = local.min(lim - local);
                if clip.looping && loop_len > 1e-9 {
                    let u = clip.start + local;
                    if u >= clip.loop_end {
                        let c = clip.wrap(u);
                        d = d.min(c - clip.loop_start).min(clip.loop_end - c);
                    } else {
                        d = d.min(clip.loop_end - u);
                    }
                }
                ramp(d / ctx.bps) * uf
            };
            let gain_at = |c: f64| -> f32 { if w.gain_env.is_empty() { a.gain } else { a.gain * NoteMpe::eval(&w.gain_env, c, 1.0) } };
            let local0 = ctx.beat0 + first as f64 * ctx.bps - origin;
            let tr = w.transpose
                + if w.transpose_env.is_empty() { 0.0 } else { NoteMpe::eval(&w.transpose_env, clip.wrap(clip.start + local0), 0.0) };
            let rate = (tr as f64 / 12.0).exp2();
            // Unwarped, or no map (legacy / tests): straight read at the clip's BPM.
            let raw = a.warp_bpm.is_none() || w.map.knots.is_empty();
            let mode = if raw { StretchMode::RePitch } else { w.mode };
            let fpb_raw = 60.0 / a.warp_bpm.unwrap_or(ctx.tempo) * sr_s;
            match mode {
                StretchMode::Complex | StretchMode::ComplexPro => {
                    let cnt = end - first;
                    let (tl, trr) = (&mut out.tmp_l[..cnt], &mut out.tmp_r[..cnt]);
                    tl.fill(0.0);
                    trr.fill(0.0);
                    let b0 = ctx.beat0 + first as f64 * ctx.bps;
                    let pos_at = |off: i64| -> Option<f64> {
                        let local = b0 + off as f64 * ctx.bps - origin;
                        if local < -EPS || local >= lim {
                            return None;
                        }
                        Some(w.map.frame_at(clip.wrap(clip.start + local.max(0.0))))
                    };
                    let fm = (mode == StretchMode::ComplexPro)
                        .then_some(Formants { amount: w.params.formants, lifter: w.params.envelope as usize });
                    let key = origin.to_bits() ^ (std::ptr::from_ref(&*a.sample) as u64);
                    out.pv.process(&a.sample, (out.stamp + first as u64, key), pos_at, rate as f32, fm, tl, trr);
                    for k in 0..cnt {
                        let local = local0 + k as f64 * ctx.bps;
                        let g = gain_at(clip.wrap(clip.start + local)) * edge_gain(local);
                        out.l[first + k] += tl[k] * g;
                        out.r[first + k] += trr[k] * g;
                    }
                }
                _ => {
                    let sc = StretchCtx { sample: &a.sample, map: &w.map, params: w.params, segs: &w.segs, rate, spb, hiq: w.hiq };
                    for i in first..end {
                        let local = ctx.beat0 + i as f64 * ctx.bps - origin;
                        let u = clip.start + local.max(0.0);
                        let c = clip.wrap(u);
                        let (x, y) = match mode {
                            _ if raw => a.sample.read(a.offset_frames + c * fpb_raw),
                            StretchMode::RePitch => sc.repitch(c),
                            StretchMode::Beats => sc.beats(c),
                            _ => sc.granular(u, |x| clip.wrap(x)),
                        };
                        let g = gain_at(c) * edge_gain(local.max(0.0));
                        out.l[i] += x * g;
                        out.r[i] += y * g;
                    }
                }
            }
        }
        ClipContent::Midi(notes) => {
            // Unwrapped play position u = start + local: sample i owns
            // [u0 + i·bps, u0 + (i+1)·bps), contiguous across segments, so
            // each note fires exactly once (also across loop wraps).
            let u0 = clip.start + (ctx.beat0 - origin);
            let u1 = u0 + n as f64 * ctx.bps;
            let lo_u = u0.max(clip.start) - EPS;
            let hi_u = (u1 - EPS).min(clip.start + lim - EPS);
            if hi_u <= lo_u {
                return true;
            }
            let first_end = if clip.looping { clip.loop_end } else { clip.start + lim };
            let mpe = &clip.extra.mpe;
            let fire = |note: &crate::song::Note, at_u: f64, cap: f64, out: &mut ClipOut| {
                if *out.n_events >= MAX_EVENTS || note.muted {
                    return;
                }
                let at = at_u - u0;
                let off = (((at + EPS) / ctx.bps).floor().max(0.0) as usize).min(n - 1);
                let len_beats = note.length.min(cap);
                let mut expr = 0u8;
                if note.mpe > 0
                    && let Some(m) = mpe.get(note.mpe as usize - 1)
                    && *out.n_expr < MAX_EXPR
                {
                    let spb = 1.0 / ctx.bps;
                    out.exprs[*out.n_expr] = Expr {
                        pitch: Curve::from_points(&m.pitch, spb),
                        slide: Curve::from_points(&m.slide, spb),
                        press: Curve::from_points(&m.pressure, spb),
                    };
                    *out.n_expr += 1;
                    expr = *out.n_expr as u8;
                }
                out.events[*out.n_events] = NoteEvent {
                    offset: off as u32,
                    pitch: note.pitch,
                    vel: note.velocity,
                    len: ((len_beats / ctx.bps) as u32).max(1),
                    expr,
                };
                *out.n_events += 1;
            };
            // First pass: content = u for u ∈ [start, first_end).
            let a0 = lo_u.max(clip.start - EPS);
            let a1 = hi_u.min(first_end - EPS);
            if a1 > a0 {
                let lo = notes.partition_point(|x| x.start < a0);
                let hi = notes.partition_point(|x| x.start < a1);
                for note in &notes[lo..hi.max(lo)] {
                    fire(note, note.start, first_end - note.start, out);
                }
            }
            // Loop passes: u ∈ [loop_end + kL, loop_end + (k+1)L) plays loop_start + (u - loop_end - kL).
            let len = clip.loop_end - clip.loop_start;
            if clip.looping && len > 1e-9 && hi_u > clip.loop_end - EPS {
                let k0 = ((lo_u - clip.loop_end) / len).floor().max(0.0) as i64;
                let k1 = ((hi_u - clip.loop_end) / len).floor() as i64;
                for k in k0..=k1 {
                    let base = clip.loop_end + k as f64 * len - clip.loop_start; // u = content + base
                    let c0 = (lo_u - base).max(clip.loop_start - EPS);
                    let c1 = (hi_u - base).min(clip.loop_end - EPS);
                    if c1 <= c0 {
                        continue;
                    }
                    let lo = notes.partition_point(|x| x.start < c0);
                    let hi = notes.partition_point(|x| x.start < c1);
                    for note in &notes[lo..hi.max(lo)] {
                        fire(note, note.start + base, clip.loop_end - note.start, out);
                    }
                }
            }
        }
    }
    true
}
