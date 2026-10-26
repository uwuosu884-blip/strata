//! Per-track real-time state: session launch state, clip playback, instrument
//! voices and the channel strip (gain/pan/mute/solo).

use crate::instruments::{PadBuf, Voices};
use crate::smoother::Smoothed;
use crate::song::{Clip, ClipContent, SongTrack};
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
}

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
}

impl TrackRt {
    pub fn new(max_block: usize, sr: f64) -> Self {
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
        }
    }

    pub fn reset(&mut self) {
        self.playing = None;
        self.pending = None;
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
        let a = (self.pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        (self.gain * a.cos() * SQRT_2, self.gain * a.sin() * SQRT_2)
    }

    /// Clip position (beats) of the playing session clip at `beat`.
    pub fn clip_pos(&self, st: &SongTrack, beat: f64) -> Option<f64> {
        let (scene, origin) = self.playing?;
        let clip = st.slots.get(scene as usize)?.as_ref()?;
        let local = (beat - origin).max(0.0);
        Some(if clip.looping { local.rem_euclid(clip.length) } else { local.min(clip.length) })
    }

    /// Render clips + instrument (pre-fader) into `self.l/r[..n]`.
    /// `clips_on` = false (monitor In) silences clip output while clips keep
    /// running; `midi_monitor` = forwarded "Track In" notes are played.
    /// Returns true if a clip covered part of the segment.
    pub fn render(&mut self, st: &SongTrack, ctx: &SegCtx, clips_on: bool, midi_monitor: bool) -> bool {
        let n = ctx.n;
        let Self { l, r, events, n_events, voices, playing, session_override, fwd, n_fwd, pads, .. } = self;
        let (l, r) = (&mut l[..n], &mut r[..n]);
        l.fill(0.0);
        r.fill(0.0);
        *n_events = 0;
        let mut produced = false;
        let mut finished = false;
        let mut covered = false;

        if ctx.playing {
            if let Some((scene, origin)) = *playing {
                match st.slots.get(scene as usize).and_then(Option::as_ref) {
                    Some(clip) => {
                        covered |= play_clip(clip, origin, clip.looping, ctx, l, r, events, n_events);
                        finished = !clip.looping && ctx.beat0 + n as f64 * ctx.bps - origin >= clip.length - EPS;
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
                        covered |= play_clip(&ac.clip, ac.start, false, ctx, l, r, events, n_events);
                    }
                }
            }
        }
        if !clips_on {
            l.fill(0.0);
            r.fill(0.0);
            *n_events = 0;
        }
        produced |= covered && clips_on;
        for f in &fwd[..*n_fwd] {
            if (f.direct || midi_monitor) && *n_events < MAX_EVENTS {
                events[*n_events] = f.ev;
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
            voices.note_on(&st.instrument, e.pitch, e.vel, e.len, true, ctx.sr);
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

/// Split samples `0..ctx.n` into windows of contiguous clip-local time and
/// call `f(local_start, first_sample, count)` for each (audio clips).
fn clip_windows(origin: f64, len: f64, looping: bool, ctx: &SegCtx, mut f: impl FnMut(f64, usize, usize)) {
    let mut i = 0;
    while i < ctx.n {
        let b = ctx.beat0 + i as f64 * ctx.bps;
        let mut local = b - origin;
        if local < 0.0 {
            if local > -EPS {
                local = 0.0;
            } else {
                // `- EPS` stops ceil() overshooting by one sample on fp noise.
                i += (((-local - EPS) / ctx.bps).ceil() as usize).max(1);
                continue;
            }
        }
        if looping {
            local = local.rem_euclid(len);
            if len - local < EPS {
                local = 0.0;
            }
        } else if local >= len - EPS {
            break;
        }
        let cnt = (((len - local - EPS) / ctx.bps).ceil() as usize).clamp(1, ctx.n - i);
        f(local, i, cnt);
        i += cnt;
    }
}

/// Play one clip into the buffers (audio) or the event list (MIDI).
/// Returns true if the clip covered any sample of this segment.
#[allow(clippy::too_many_arguments)]
fn play_clip(
    clip: &Clip,
    origin: f64,
    looping: bool,
    ctx: &SegCtx,
    l: &mut [f32],
    r: &mut [f32],
    events: &mut [NoteEvent; MAX_EVENTS],
    n_events: &mut usize,
) -> bool {
    let mut covered = false;
    match &clip.content {
        ClipContent::Audio(a) => {
            let frames_per_beat = 60.0 / a.warp_bpm.unwrap_or(ctx.tempo) * a.sample.sample_rate as f64;
            clip_windows(origin, clip.length, looping, ctx, |local, first, cnt| {
                covered = true;
                for k in 0..cnt {
                    let lb = local + k as f64 * ctx.bps;
                    let (x, y) = a.sample.read(a.offset_frames + lb * frames_per_beat);
                    let edge = (lb / ctx.bps).min((clip.length - lb) / ctx.bps);
                    let g = if edge < DECLICK { (edge / DECLICK) as f32 * a.gain } else { a.gain };
                    l[first + k] += x * g;
                    r[first + k] += y * g;
                }
            });
        }
        ClipContent::Midi(notes) => {
            // Unwrapped clip time: sample i owns [u0 + i·bps, u0 + (i+1)·bps).
            // Segments are contiguous in this time, so each note fires exactly once.
            let u0 = ctx.beat0 - origin;
            let u1 = u0 + ctx.n as f64 * ctx.bps;
            let lo_u = u0.max(0.0) - EPS;
            let hi_u = if looping { u1 - EPS } else { (u1 - EPS).min(clip.length - EPS) };
            if hi_u <= lo_u {
                return false;
            }
            covered = true;
            let len = clip.length;
            let (k0, k1) = if looping { ((lo_u / len).floor() as i64, (hi_u / len).floor() as i64) } else { (0, 0) };
            for k in k0..=k1 {
                let base = k as f64 * len;
                let lo = notes.partition_point(|n| n.start < lo_u - base);
                let hi = notes.partition_point(|n| n.start < (hi_u - base).min(len));
                for note in &notes[lo..hi.max(lo)] {
                    if *n_events >= MAX_EVENTS {
                        return covered;
                    }
                    let at = base + note.start - u0;
                    let off = (((at + EPS) / ctx.bps).floor().max(0.0) as usize).min(ctx.n - 1);
                    let len_beats = note.length.min(len - note.start);
                    events[*n_events] =
                        NoteEvent { offset: off as u32, pitch: note.pitch, vel: note.velocity, len: ((len_beats / ctx.bps) as u32).max(1) };
                    *n_events += 1;
                }
            }
        }
    }
    covered
}
