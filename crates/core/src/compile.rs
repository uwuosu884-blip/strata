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
        p::ClipContent::Midi { notes } => e::ClipContent::Midi(notes.clone()),
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

/// Build the engine song. Missing samples start loading in the background;
/// recompile when [`SampleCache::poll`] reports arrivals.
pub fn compile(project: &Project, cache: &mut SampleCache, slots: &mut RtSlots) -> e::Song {
    slots.retain(project);
    let tracks = project
        .tracks
        .iter()
        .filter_map(|t| {
            let rt = slots.assign(t.id)?; // > MAX_TRACKS tracks: extra ones are silent
            Some(e::SongTrack {
                rt,
                gain: db_to_gain(t.gain_db),
                pan: t.pan.clamp(-1.0, 1.0),
                mute: t.mute,
                solo: t.solo,
                monitor: t.monitor,
                instrument: instrument(&t.instrument, cache),
                slots: t.slots.iter().map(|s| s.as_ref().map(|c| clip(c, cache))).collect(),
                arrangement: t.arrangement.iter().map(|a| e::ArrangementClip { start: a.start, clip: clip(&a.clip, cache) }).collect(),
            })
        })
        .collect();
    e::Song { tracks }
}

#[cfg(test)]
mod tests {
    use super::*;

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
