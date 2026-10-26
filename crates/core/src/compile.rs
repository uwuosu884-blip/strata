//! Project (editable document) → Song (immutable, audio-thread snapshot).
//! Runs on the GUI thread after every structural edit; cheap (samples are Arc'd).

use crate::samples::SampleCache;
use daw_engine as e;
use daw_project::{self as p, Project, db_to_gain};
use std::collections::{HashMap, VecDeque};

/// Stable mapping track id → engine track slot (`rt`). Slots of deleted
/// tracks are reused oldest-first, so a stale voice tail never lands on a new track.
#[derive(Debug)]
pub struct RtSlots {
    map: HashMap<u64, u16>,
    free: VecDeque<u16>,
}

impl Default for RtSlots {
    fn default() -> Self {
        Self { map: HashMap::new(), free: (0..e::MAX_TRACKS as u16).collect() }
    }
}

impl RtSlots {
    pub fn get(&self, track_id: u64) -> Option<u16> {
        self.map.get(&track_id).copied()
    }

    fn assign(&mut self, track_id: u64) -> Option<u16> {
        if let Some(rt) = self.map.get(&track_id) {
            return Some(*rt);
        }
        let rt = self.free.pop_front()?;
        self.map.insert(track_id, rt);
        Some(rt)
    }

    fn retain(&mut self, project: &Project) {
        let free = &mut self.free;
        self.map.retain(|id, rt| {
            let keep = project.tracks.iter().any(|t| t.id == *id);
            if !keep {
                free.push_back(*rt);
            }
            keep
        });
    }
}

/// Shared compile inputs for clips.
struct ClipCtx<'a> {
    grooves: &'a [p::clipx::Groove],
    amount: f32,
}

fn env_points(c: &p::Clip, t: p::clipx::EnvTarget) -> (Vec<(f64, f32)>, f64) {
    match c.envelope(t) {
        Some(e) => (e.points.iter().map(|q| (q.beat, q.value)).collect(), if e.unlinked { e.loop_len.max(1.0 / 64.0) } else { 0.0 }),
        None => (Vec::new(), 0.0),
    }
}

fn follow(a: p::clipx::FollowAction, jump: u16) -> e::Follow {
    use p::clipx::FollowAction as F;
    match a {
        F::NoAction => e::Follow::None,
        F::Stop => e::Follow::Stop,
        F::PlayAgain => e::Follow::Again,
        F::Previous => e::Follow::Prev,
        F::Next => e::Follow::Next,
        F::First => e::Follow::First,
        F::Last => e::Follow::Last,
        F::Any => e::Follow::Any,
        F::Other => e::Follow::Other,
        F::Jump => e::Follow::Jump(jump.saturating_sub(1)), // UI is 1-based
    }
}

/// Window of content beats that can sound (start marker / loop brace).
fn play_window(c: &p::Clip) -> (f64, f64) {
    if c.looping {
        let (a, b) = c.loop_range();
        (c.start.min(a), b)
    } else {
        (c.start, c.start + c.length)
    }
}

/// Beat → frame map for an audio clip (markers, else straight at its BPM).
pub fn warp_map(c: &p::Clip, offset: f64, warp_bpm: Option<f64>, sr: f64) -> e::WarpMap {
    let fpb = warp_bpm.map_or(sr / 2.0, |b| sr * 60.0 / b.max(1.0));
    let m: Vec<(f64, f64)> = c.audio.markers.iter().map(|m| (m.beat, m.frame)).collect();
    e::WarpMap::from_markers(&m, offset, fpb)
}

fn clip(c: &p::Clip, cache: &mut SampleCache, cx: &ClipCtx) -> e::Clip {
    let groove = c.groove.and_then(|id| cx.grooves.iter().find(|g| g.id == id));
    let (w0, w1) = play_window(c);
    let mut extra = e::ClipExtra::default();
    let content = match &c.content {
        // Deactivated clips keep their timing (session launch) but play nothing.
        _ if c.muted => e::ClipContent::Midi(Vec::new()),
        // Left-trimmed / outside the markers and deactivated notes are not played.
        p::ClipContent::Midi { notes } => {
            let mut v: Vec<e::Note> = notes
                .iter()
                .enumerate()
                .filter(|(_, n)| !n.muted && n.start >= w0 - 1e-9 && n.start < w1 && (c.start > 0.0 || n.start >= 0.0))
                .map(|(k, n)| {
                    let mut n = match groove {
                        Some(g) => g.apply(n, cx.amount, k as u64),
                        None => *n,
                    };
                    if n.mpe as usize > c.mpe.len() {
                        n.mpe = 0;
                    }
                    n
                })
                .collect();
            v.sort_unstable_by(|a, b| a.start.total_cmp(&b.start));
            extra.mpe = c.mpe.clone();
            e::ClipContent::Midi(v)
        }
        p::ClipContent::Audio { sample, offset_frames, warp_bpm, gain_db } => match cache.get(sample) {
            Some(buf) => {
                let sr = buf.sample_rate as f64;
                let a = &c.audio;
                let mut map = warp_map(c, *offset_frames, *warp_bpm, sr);
                // Groove on audio: move grid points of the warp map.
                if let Some(g) = groove.filter(|g| g.base > 0.0 && warp_bpm.is_some()) {
                    let n0 = (w0 / g.base).floor() as i64;
                    let n1 = ((w1 / g.base).ceil() as i64).min(n0 + 4096);
                    let knots: Vec<(f64, f64)> = (n0..=n1)
                        .map(|i| {
                            let b = i as f64 * g.base;
                            (b + g.shift_at(b, cx.amount), map.frame_at(b))
                        })
                        .collect();
                    map = e::WarpMap { knots, ..map };
                }
                let mode = match a.mode {
                    p::clipx::WarpMode::Beats => e::StretchMode::Beats,
                    p::clipx::WarpMode::Tones => e::StretchMode::Tones,
                    p::clipx::WarpMode::Texture => e::StretchMode::Texture,
                    p::clipx::WarpMode::RePitch => e::StretchMode::RePitch,
                    p::clipx::WarpMode::Complex => e::StretchMode::Complex,
                    p::clipx::WarpMode::ComplexPro => e::StretchMode::ComplexPro,
                };
                let wp = &a.params;
                let params = e::StretchParams {
                    fade: (1.0 - wp.envelope / 100.0).clamp(0.0, 1.0),
                    loop_mode: match wp.loop_mode {
                        p::clipx::TransientLoop::Off => e::LoopMode::Off,
                        p::clipx::TransientLoop::Forward => e::LoopMode::Forward,
                        p::clipx::TransientLoop::BackAndForth => e::LoopMode::BackAndForth,
                    },
                    // Live grain 12..100 → ~20..160 ms.
                    grain: if a.mode == p::clipx::WarpMode::Texture { wp.grain_texture } else { wp.grain_tones }.clamp(1.0, 100.0) as f64
                        * 0.0016,
                    flux: (wp.flux / 100.0).clamp(0.0, 1.0),
                    formants: (wp.formants / 100.0).clamp(0.0, 1.0),
                    envelope: wp.cpro_envelope.clamp(8.0, 256.0),
                };
                // Beats segments: transients (edited or detected) or a fixed grid.
                let segs: Vec<f64> = match wp.preserve {
                    _ if a.mode != p::clipx::WarpMode::Beats => Vec::new(),
                    p::clipx::Preserve::Transients => {
                        let t = a.transients.as_deref().unwrap_or(&buf.transients);
                        let mut v: Vec<f64> = t.iter().map(|&f| map.beat_at(f)).collect();
                        v.sort_by(f64::total_cmp);
                        v
                    }
                    p::clipx::Preserve::Grid(g) => {
                        let g = g.max(1.0 / 64.0);
                        let n0 = (w0.min(0.0) / g).floor() as i64;
                        let n1 = ((w1 / g).ceil() as i64).min(n0 + 8192);
                        (n0..=n1).map(|i| i as f64 * g).collect()
                    }
                };
                let gain_env = env_points(c, p::clipx::EnvTarget::ClipGain).0;
                let transpose_env = env_points(c, p::clipx::EnvTarget::Transpose).0;
                let warp = e::WarpSetup {
                    map,
                    mode,
                    params,
                    segs,
                    transpose: a.transpose as f32 + a.detune / 100.0,
                    hiq: a.hiq,
                    fade: a.fade,
                    gain_env,
                    transpose_env,
                };
                e::ClipContent::Audio(e::AudioClip {
                    sample: buf,
                    offset_frames: *offset_frames,
                    warp_bpm: *warp_bpm,
                    gain: db_to_gain(*gain_db),
                    warp,
                })
            }
            // Not loaded yet: silent placeholder keeps launch timing intact.
            None => e::ClipContent::Midi(Vec::new()),
        },
    };
    let length = c.length.max(1.0 / 64.0);
    let (ls, le) = c.loop_range();
    use p::clipx::EnvTarget as T;
    let (vol, l0) = env_points(c, T::Volume);
    let (pan, l1) = env_points(c, T::Pan);
    let (bend, l2) = env_points(c, T::PitchBend);
    let (modw, l3) = env_points(c, T::ModWheel);
    let fa = &c.launch.follow;
    let cycle = if c.looping { le - ls } else { length };
    extra.launch = e::Launch {
        mode: match c.launch.mode {
            p::clipx::LaunchMode::Trigger => e::LaunchMode::Trigger,
            p::clipx::LaunchMode::Gate => e::LaunchMode::Gate,
            p::clipx::LaunchMode::Toggle => e::LaunchMode::Toggle,
            p::clipx::LaunchMode::Repeat => e::LaunchMode::Repeat,
        },
        quantize: c.launch.quantize,
        legato: c.launch.legato,
        velocity: (c.launch.velocity / 100.0).clamp(0.0, 1.0),
        follow: fa.enabled.then(|| {
            let time = if fa.linked { cycle * fa.multiplier.max(1) as f64 } else { fa.time };
            (follow(fa.a, fa.jump_a), follow(fa.b, fa.jump_b), fa.chance_a.min(100) as f32 / 100.0, time.max(1.0 / 64.0))
        }),
    };
    extra.vol_env = vol;
    extra.pan_env = pan;
    extra.bend_env = bend;
    extra.mod_env = modw;
    extra.env_loop = [l0, l1, l2, l3];
    extra.program = [c.program.bank, c.program.sub, c.program.program];
    e::Clip { length, looping: c.looping, content, start: c.start, loop_start: ls, loop_end: le, extra: Box::new(extra) }
}

/// Tempo leaders (Live 9.4): the bottom-most playing arrangement audio clip
/// marked Leader sets the tempo so it plays unstretched. Returns tempo steps.
fn leader_tempo(project: &Project, cache: &mut SampleCache) -> Vec<(f64, f64)> {
    // (priority, start, end, steps) per leader clip; later tracks win.
    type Span = (usize, f64, f64, Vec<(f64, f64)>);
    let mut spans: Vec<Span> = Vec::new();
    for (ti, t) in project.tracks.iter().enumerate() {
        for a in t.arrangement.iter().filter(|a| a.clip.audio.lead && !a.clip.muted) {
            let p::ClipContent::Audio { sample, offset_frames, warp_bpm: Some(bpm), .. } = &a.clip.content else { continue };
            let Some(buf) = cache.get(sample) else { continue };
            let c = &a.clip;
            let map = warp_map(c, *offset_frames, Some(*bpm), buf.sample_rate as f64);
            let sr = buf.sample_rate as f64;
            let tempo_at = |u: f64| 60.0 * sr / map.slope_at(u).max(1e-6);
            // Breakpoints: knot beats crossed during playback (bounded).
            let mut steps = vec![(a.start, tempo_at(c.content_at(0.0).unwrap_or(0.0)))];
            let mut local = 0.0;
            for _ in 0..512 {
                let Some(u) = c.content_at(local) else { break };
                let next_knot = map.knots.iter().map(|k| k.0).find(|&b| b > u + 1e-9);
                let to_loop_end = if c.looping { c.loop_range().1 - u } else { f64::INFINITY };
                let step = next_knot.map_or(f64::INFINITY, |b| b - u).min(to_loop_end);
                if !step.is_finite() || local + step >= c.length {
                    break;
                }
                local += step.max(1e-6);
                if let Some(u2) = c.content_at(local) {
                    steps.push((a.start + local, tempo_at(u2 + 1e-9)));
                }
            }
            spans.push((ti, a.start, a.start + c.length, steps));
        }
    }
    if spans.is_empty() {
        return Vec::new();
    }
    let base = project.tempo_bpm;
    let mut cuts: Vec<f64> = spans.iter().flat_map(|s| s.3.iter().map(|x| x.0).chain([s.1, s.2])).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|b, a| (*b - *a).abs() < 1e-9);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for &b in &cuts {
        let top = spans.iter().filter(|s| s.1 <= b + 1e-9 && b < s.2 - 1e-9).max_by_key(|s| s.0);
        let bpm = top.map_or(base, |s| s.3.iter().rev().find(|x| x.0 <= b + 1e-9).map_or(base, |x| x.1)).clamp(20.0, 999.0);
        if out.last().is_none_or(|l| (l.1 - bpm).abs() > 1e-6) {
            out.push((b, bpm));
        }
    }
    out
}

fn instrument(def: &p::InstrumentDef, cache: &mut SampleCache) -> e::Instrument {
    match def {
        p::InstrumentDef::None => e::Instrument::None,
        p::InstrumentDef::Synth(s) => e::Instrument::Synth(*s),
        p::InstrumentDef::Sampler { sample, params } => {
            e::Instrument::Sampler { sample: sample.as_deref().and_then(|k| cache.get(k)), params: *params }
        }
        p::InstrumentDef::Drums { pads } => e::Instrument::Drums(
            pads.iter()
                .take(e::DRUM_PADS)
                .map(|d| e::DrumPad {
                    name: d.name.clone(),
                    sample: d.sample.as_deref().and_then(|k| cache.get(k)),
                    gain: d.gain,
                    tune_semitones: d.tune_semitones,
                    choke: d.choke,
                })
                .collect(),
        ),
    }
}

/// Hardware MIDI ports the engine knows, by name; the index is the engine port.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IoMap {
    pub midi_in: Vec<String>,
    pub midi_out: Vec<String>,
}

impl IoMap {
    fn port(list: &[String], name: &str) -> u8 {
        list.iter().position(|n| n == name).filter(|&i| i < e::MIDI_NO_PORT as usize).map_or(e::MIDI_NO_PORT, |i| i as u8)
    }
}

/// Build the engine song. Missing samples start loading in the background;
/// recompile when [`SampleCache::poll`] reports arrivals.
pub fn compile(project: &Project, cache: &mut SampleCache, slots: &mut RtSlots) -> e::Song {
    compile_io(project, cache, slots, &IoMap::default())
}

/// [`compile`] with MIDI port names resolved against `io`.
pub fn compile_io(project: &Project, cache: &mut SampleCache, slots: &mut RtSlots, io: &IoMap) -> e::Song {
    slots.retain(project);
    let cx = ClipCtx { grooves: &project.grooves, amount: project.groove_amount.clamp(0.0, 1.3) };
    // Slots first: routing may point at any track.
    for t in &project.tracks {
        slots.assign(t.id); // > MAX_TRACKS tracks: extra ones are silent
    }
    let tracks = project
        .tracks
        .iter()
        .filter_map(|t| {
            let rt = slots.get(t.id)?;
            Some(e::SongTrack {
                input: input(t, slots, io),
                output: output(t, slots, io),
                rt,
                gain: db_to_gain(t.gain_db),
                pan: t.pan.clamp(-1.0, 1.0),
                mute: t.mute,
                solo: t.solo,
                arm: t.arm && t.has_input(),
                monitor: t.monitor_mode(),
                instrument: instrument(&t.instrument, cache),
                slots: t.slots.iter().map(|s| s.as_ref().map(|c| clip(c, cache, &cx))).collect(),
                arrangement: t
                    .arrangement
                    .iter()
                    .filter(|a| !a.clip.muted)
                    .map(|a| e::ArrangementClip { start: a.start, clip: clip(&a.clip, cache, &cx) })
                    .collect(),
                ..Default::default()
            })
        })
        .collect();
    let mut song = e::Song::new(tracks);
    song.master_out = (project.master_out_ch, project.master_out_stereo);
    song.base_tempo = project.tempo_bpm;
    song.tempo_map = leader_tempo(project, cache);
    song
}

fn input(t: &p::Track, slots: &RtSlots, io: &IoMap) -> e::TrackInput {
    let ch = t.midi_channel;
    match t.kind {
        p::TrackKind::Group => e::TrackInput::None,
        p::TrackKind::Audio => match t.audio_in {
            p::AudioIn::None => e::TrackInput::None,
            p::AudioIn::Ext { ch, stereo } => e::TrackInput::Ext { ch, stereo },
            p::AudioIn::Resampling => e::TrackInput::Resampling,
            p::AudioIn::Track { id, tap } => slots.get(id).map_or(e::TrackInput::None, |rt| e::TrackInput::Track { rt, tap }),
        },
        p::TrackKind::Midi => match &t.midi_in {
            p::MidiIn::None => e::TrackInput::None,
            p::MidiIn::AllIns => e::TrackInput::Midi { port: e::MIDI_ALL_INS, channel: ch },
            p::MidiIn::Keyboard => e::TrackInput::Midi { port: e::MIDI_KEYBOARD, channel: ch },
            p::MidiIn::Port(name) => e::TrackInput::Midi { port: IoMap::port(&io.midi_in, name), channel: ch },
            p::MidiIn::Track(id) => slots.get(*id).map_or(e::TrackInput::None, |rt| e::TrackInput::MidiTrack { rt }),
        },
    }
}

fn output(t: &p::Track, slots: &RtSlots, io: &IoMap) -> e::TrackOutput {
    if t.sends_midi() {
        return match &t.midi_out {
            p::MidiOut::None => e::TrackOutput::None,
            p::MidiOut::Port { name, channel } => match IoMap::port(&io.midi_out, name) {
                e::MIDI_NO_PORT => e::TrackOutput::None,
                port => e::TrackOutput::MidiPort { port, channel: *channel },
            },
            p::MidiOut::Track { id, instrument } => {
                slots.get(*id).map_or(e::TrackOutput::None, |rt| e::TrackOutput::MidiTrack { rt, instrument: *instrument })
            }
        };
    }
    match t.audio_out {
        p::AudioOut::Default => t.group.and_then(|g| slots.get(g)).map_or(e::TrackOutput::Master, e::TrackOutput::Track),
        p::AudioOut::Master => e::TrackOutput::Master,
        p::AudioOut::Ext { ch, stereo } => e::TrackOutput::Ext { ch, stereo },
        p::AudioOut::Track(id) => slots.get(id).map_or(e::TrackOutput::Master, e::TrackOutput::Track),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_children_route_to_the_group_bus() {
        let mut proj = Project::new_default();
        let g = proj.group_tracks(&[1, 2]).unwrap();
        let mut cache = SampleCache::new(48_000);
        let mut slots = RtSlots::default();
        let mut song = compile(&proj, &mut cache, &mut slots);
        song.prepare();
        let grt = song.tracks[g].rt;
        assert_eq!(song.tracks[g].output, e::TrackOutput::Master);
        assert_eq!(song.tracks[g + 1].output, e::TrackOutput::Track(grt));
        assert_eq!(song.tracks[g + 2].output, e::TrackOutput::Track(grt));
        assert_eq!(song.tracks[0].output, e::TrackOutput::Master);
        assert_eq!(song.tracks.last().unwrap().output, e::TrackOutput::Master);
    }

    #[test]
    fn routing_resolves_ids_ports_and_defaults() {
        let mut proj = Project::new_default();
        let a = proj.add_track(p::TrackKind::Audio);
        let m = proj.add_track(p::TrackKind::Midi);
        let (id0, ida) = (proj.tracks[0].id, proj.tracks[a].id);
        proj.tracks[a].audio_in = p::AudioIn::Track { id: id0, tap: p::Tap::Pad(3) };
        proj.tracks[a].audio_out = p::AudioOut::Ext { ch: 2, stereo: false };
        proj.tracks[m].midi_in = p::MidiIn::Port("Keys B".into());
        proj.tracks[m].midi_channel = 10;
        proj.tracks[m].midi_out = p::MidiOut::Port { name: "Synth".into(), channel: 3 };
        proj.master_out_ch = 2;
        let io = IoMap { midi_in: vec!["Keys A".into(), "Keys B".into()], midi_out: vec!["Synth".into()] };
        let mut cache = SampleCache::new(48_000);
        let mut slots = RtSlots::default();
        let song = compile_io(&proj, &mut cache, &mut slots, &io);
        let rt0 = slots.get(id0).unwrap();
        assert_eq!(song.tracks[a].input, e::TrackInput::Track { rt: rt0, tap: p::Tap::Pad(3) });
        assert_eq!(song.tracks[a].output, e::TrackOutput::Ext { ch: 2, stereo: false });
        assert_eq!(song.tracks[a].monitor, e::Monitor::Off, "audio default Off");
        assert_eq!(song.tracks[m].input, e::TrackInput::Midi { port: 1, channel: 10 });
        assert_eq!(song.tracks[m].output, e::TrackOutput::MidiPort { port: 0, channel: 3 }, "no instrument → MIDI To");
        assert_eq!(song.tracks[m].monitor, e::Monitor::Auto, "MIDI default Auto");
        assert_eq!(song.master_out, (2, true));
        // Missing port → never matches; deleted target → normalised away.
        let song = compile_io(&proj, &mut cache, &mut slots, &IoMap::default());
        assert_eq!(song.tracks[m].input, e::TrackInput::Midi { port: e::MIDI_NO_PORT, channel: 10 });
        assert_eq!(song.tracks[m].output, e::TrackOutput::None);
        proj.tracks[m].audio_in = p::AudioIn::Track { id: ida, tap: p::Tap::PostFx };
        proj.tracks.remove(0);
        proj.normalize();
        assert_eq!(proj.tracks[a - 1].audio_in, p::AudioIn::None);
    }

    #[test]
    fn compiles_default_project_with_stable_slots() {
        let mut proj = Project::new_default();
        proj.new_midi_clip(0, 0, 4.0);
        let mut cache = SampleCache::new(48_000);
        let mut slots = RtSlots::default();
        let song = compile(&proj, &mut cache, &mut slots);
        assert_eq!(song.tracks.len(), 4);
        assert_eq!(song.tracks.iter().map(|t| t.rt).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert!(matches!(&song.tracks[0].instrument, e::Instrument::Drums(p) if p.iter().all(|d| d.sample.is_some())));
        assert!(song.tracks[0].slots[0].is_some() && song.tracks[0].slots[1].is_none());

        // Delete track 1, add one: the new track must not reuse slot 1 first.
        let drum_id = proj.tracks[0].id;
        proj.tracks.remove(1);
        proj.add_track(p::TrackKind::Audio);
        let song = compile(&proj, &mut cache, &mut slots);
        assert_eq!(song.tracks[0].rt, slots.get(drum_id).unwrap());
        assert_eq!(song.tracks.last().unwrap().rt, 4);
    }

    #[test]
    fn notes_past_the_clip_end_are_silent() {
        let mut proj = Project::new_default();
        let n = |start| p::Note { start, length: 0.5, pitch: 60, velocity: 100, muted: false, mpe: 0 };
        proj.tracks[1].slots[0] = Some(p::Clip {
            length: 4.0,
            content: p::ClipContent::Midi { notes: vec![n(1.0), n(3.9), n(4.0), n(6.0)] },
            ..Default::default()
        });
        let mut cache = SampleCache::new(48_000);
        let song = compile(&proj, &mut cache, &mut RtSlots::default());
        let c = song.tracks[1].slots[0].as_ref().unwrap();
        assert!(matches!(&c.content, e::ClipContent::Midi(v) if v.len() == 2));
    }

    #[test]
    fn missing_audio_becomes_silent_placeholder() {
        let mut proj = Project::new_default();
        proj.tracks[3].slots[0] = Some(p::Clip {
            length: 8.0,
            content: p::ClipContent::Audio { sample: "/nonexistent/x.wav".into(), offset_frames: 0.0, warp_bpm: None, gain_db: 0.0 },
            ..Default::default()
        });
        let mut cache = SampleCache::new(48_000);
        let song = compile(&proj, &mut cache, &mut RtSlots::default());
        let c = song.tracks[3].slots[0].as_ref().unwrap();
        assert_eq!(c.length, 8.0);
        assert!(matches!(&c.content, e::ClipContent::Midi(n) if n.is_empty()));
    }

    #[test]
    fn clip_view_props_reach_the_engine() {
        let mut proj = Project::new_default();
        let n = |start| p::Note { start, length: 0.25, pitch: 60, velocity: 100, muted: false, mpe: 0 };
        let mut c =
            p::Clip { length: 4.0, content: p::ClipContent::Midi { notes: vec![n(0.0), n(0.5), n(5.0), n(9.0)] }, ..Default::default() };
        c.start = 0.5;
        c.loop_start = 0.0;
        c.loop_end = 6.0;
        c.launch.mode = p::clipx::LaunchMode::Gate;
        c.launch.follow = p::clipx::FollowActions {
            enabled: true,
            a: p::clipx::FollowAction::Jump,
            jump_a: 3,
            linked: true,
            multiplier: 2,
            ..Default::default()
        };
        c.program.program = Some(7);
        c.envelope_mut(p::clipx::EnvTarget::Volume).points = vec![p::clipx::EnvPoint { beat: 0.0, value: 0.5 }];
        // Groove: 8th swing 66 % moves the off-beat note.
        proj.grooves.push(p::clipx::Groove { id: 9, ..p::clipx::Groove::swing(0.5, 66.0, "s") });
        c.groove = Some(9);
        proj.tracks[1].slots[0] = Some(c);
        let mut cache = SampleCache::new(48_000);
        let song = compile(&proj, &mut cache, &mut RtSlots::default());
        let ec = song.tracks[1].slots[0].as_ref().unwrap();
        assert_eq!((ec.start, ec.loop_start, ec.loop_end), (0.5, 0.0, 6.0));
        let e::ClipContent::Midi(v) = &ec.content else { panic!() };
        let starts: Vec<f64> = v.iter().map(|n| (n.start * 1000.0).round() / 1000.0).collect();
        assert_eq!(starts, vec![0.0, 0.66, 5.0], "outside the loop dropped, groove applied");
        assert_eq!(ec.extra.launch.mode, e::LaunchMode::Gate);
        assert_eq!(ec.extra.launch.follow, Some((e::Follow::Jump(2), e::Follow::None, 1.0, 12.0)));
        assert_eq!(ec.extra.program, [None, None, Some(7)]);
        assert_eq!(ec.extra.vol_env, vec![(0.0, 0.5)]);
    }

    #[test]
    fn leader_clip_sets_tempo_map() {
        let mut proj = Project::new_default();
        let mut cache = SampleCache::new(48_000);
        cache.insert("k", e::SampleBuffer::mono("k", 48_000, vec![0.0; 48_000 * 4]));
        // Markers: 2 beats over 1 s (120 BPM), then 2 beats over 2 s (60 BPM).
        let mut c = p::Clip {
            length: 4.0,
            looping: false,
            content: p::ClipContent::Audio { sample: "k".into(), offset_frames: 0.0, warp_bpm: Some(120.0), gain_db: 0.0 },
            ..Default::default()
        };
        c.audio.lead = true;
        c.audio.markers = vec![
            p::clipx::WarpMarker { beat: 0.0, frame: 0.0 },
            p::clipx::WarpMarker { beat: 2.0, frame: 48_000.0 },
            p::clipx::WarpMarker { beat: 4.0, frame: 144_000.0 },
        ];
        proj.tempo_bpm = 100.0;
        proj.tracks[3].arrangement = vec![p::ArrangementClip { start: 8.0, clip: c }];
        let song = compile(&proj, &mut cache, &mut RtSlots::default());
        assert_eq!(song.base_tempo, 100.0);
        let m: Vec<(f64, f64)> = song.tempo_map.iter().map(|&(b, t)| (b, (t * 1000.0).round() / 1000.0)).collect();
        assert_eq!(m, vec![(8.0, 120.0), (10.0, 60.0), (12.0, 100.0)]);
    }
}
