//! Per-track real-time state: session launch state, clip playback, instrument
//! voices and the channel strip (gain/pan/mute/solo).

use crate::instruments::Voices;
use crate::smoother::Smoothed;
use crate::song::{Clip, ClipContent, SongTrack};
use std::f32::consts::{FRAC_PI_4, SQRT_2};

const MAX_EVENTS: usize = 512;
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
struct NoteEvent {
    offset: u32,
    pitch: u8,
    vel: u8,
    len: u32,
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
    pub monitor: bool,
    gl: Smoothed,
    gr: Smoothed,
    /// (scene, song beat where clip position 0 is)
    pub playing: Option<(u32, f64)>,
    pub pending: Option<Pending>,
    /// A session clip took over this track → arrangement muted until "Back to Arrangement".
    pub session_override: bool,
    pub voices: Voices,
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    events: [NoteEvent; MAX_EVENTS],
    n_events: usize,
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
            monitor: false,
            gl: Smoothed::new(1.0, sr, 0.01),
            gr: Smoothed::new(1.0, sr, 0.01),
            playing: None,
            pending: None,
            session_override: false,
            voices: Voices::default(),
            l: vec![0.0; max_block],
            r: vec![0.0; max_block],
            events: [NoteEvent::default(); MAX_EVENTS],
            n_events: 0,
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
        self.monitor = st.monitor;
    }

    /// Equal-power pan normalised to unity at centre.
    fn target_gains(&self, any_solo: bool) -> (f32, f32) {
        if self.mute || (any_solo && !self.solo) {
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

    /// Render this track's own output (pre-fader) into `self.l/r[..n]`.
    pub fn render(&mut self, st: &SongTrack, ctx: &SegCtx, input: Option<(&[f32], &[f32])>) {
        let n = ctx.n;
        let Self { l, r, events, n_events, voices, playing, session_override, .. } = self;
        let (l, r) = (&mut l[..n], &mut r[..n]);
        l.fill(0.0);
        r.fill(0.0);
        *n_events = 0;
        let mut produced = false;
        let mut finished = false;

        if ctx.playing {
            if let Some((scene, origin)) = *playing {
                match st.slots.get(scene as usize).and_then(Option::as_ref) {
                    Some(clip) => {
                        produced |= play_clip(clip, origin, clip.looping, ctx, l, r, events, n_events);
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
                        produced |= play_clip(&ac.clip, ac.start, false, ctx, l, r, events, n_events);
                    }
                }
            }
        }

        // Instrument: render between note events for sample accuracy.
        let evs = &mut events[..*n_events];
        evs.sort_unstable_by_key(|e| e.offset); // in-place, never allocates
        let mut pos = 0;
        for e in evs.iter() {
            let off = (e.offset as usize).min(n);
            if off > pos {
                voices.render(&st.instrument, &mut l[pos..off], &mut r[pos..off], ctx.sr);
                pos = off;
            }
            voices.note_on(&st.instrument, e.pitch, e.vel, e.len, true, ctx.sr);
        }
        if voices.is_active() {
            voices.render(&st.instrument, &mut l[pos..], &mut r[pos..], ctx.sr);
            produced = true;
        }

        if let Some((il, ir)) = input {
            for i in 0..n {
                l[i] += il[i];
                r[i] += ir[i];
            }
            produced = true;
        }

        if finished {
            *playing = None;
            voices.release_clip_notes();
        }
        self.silent = !produced;
    }

    /// Apply gain/pan (smoothed) and add into the master bus.
    pub fn mix_into(&mut self, ml: &mut [f32], mr: &mut [f32], any_solo: bool) {
        let (tl, tr) = self.target_gains(any_solo);
        self.gl.set_target(tl);
        self.gr.set_target(tr);
        let n = ml.len();
        if self.silent {
            // Keep smoothers moving so a later sound starts at the right gain.
            self.gl.skip(n);
            self.gr.skip(n);
            return;
        }
        let mut pk = [0.0f32; 2];
        for i in 0..n {
            let a = self.l[i] * self.gl.next();
            let b = self.r[i] * self.gr.next();
            ml[i] += a;
            mr[i] += b;
            pk[0] = pk[0].max(a.abs());
            pk[1] = pk[1].max(b.abs());
        }
        self.peak[0] = self.peak[0].max(pk[0]);
        self.peak[1] = self.peak[1].max(pk[1]);
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
