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

fn clip(c: &p::Clip, cache: &mut SampleCache) -> e::Clip {
    let content = match &c.content {
        // Deactivated clips keep their timing (session launch) but play nothing.
        _ if c.muted => e::ClipContent::Midi(Vec::new()),
        // Left-trimmed (negative start) and deactivated notes are not played.
        p::ClipContent::Midi { notes } => {
            e::ClipContent::Midi(notes.iter().filter(|n| !n.muted && n.start >= 0.0 && n.start < c.length).copied().collect())
        }
        p::ClipContent::Audio { sample, offset_frames, warp_bpm, gain_db } => match cache.get(sample) {
            Some(buf) => e::ClipContent::Audio(e::AudioClip {
                sample: buf,
                offset_frames: *offset_frames,
                warp_bpm: *warp_bpm,
                gain: db_to_gain(*gain_db),
            }),
            // Not loaded yet: silent placeholder keeps launch timing intact.
            None => e::ClipContent::Midi(Vec::new()),
        },
    };
    e::Clip { length: c.length.max(1.0 / 64.0), looping: c.looping, content }
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
                slots: t.slots.iter().map(|s| s.as_ref().map(|c| clip(c, cache))).collect(),
                arrangement: t
                    .arrangement
                    .iter()
                    .filter(|a| !a.clip.muted)
                    .map(|a| e::ArrangementClip { start: a.start, clip: clip(&a.clip, cache) })
                    .collect(),
                ..Default::default()
            })
        })
        .collect();
    let mut song = e::Song::new(tracks);
    song.master_out = (project.master_out_ch, project.master_out_stereo);
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
        let n = |start| p::Note { start, length: 0.5, pitch: 60, velocity: 100, muted: false };
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
}
