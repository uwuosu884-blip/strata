//! Clip View operations (undoable): groove commit, duplicate loop, crop,
//! reverse, warp marker quantize / warp-as, MIDI Tools apply / restore.

use crate::app::{ClipRef, DawApp};
use crate::ui::clip::region;
use daw_project::clipx::WarpMarker;
use daw_project::miditools::{self as mt, Ctx};
use daw_project::{Clip, ClipContent, Note};

impl DawApp {
    fn audio_map_of(&self, c: &Clip) -> Option<(daw_engine::WarpMap, std::sync::Arc<daw_engine::SampleBuffer>)> {
        let ClipContent::Audio { sample, offset_frames, warp_bpm, .. } = &c.content else { return None };
        let buf = self.cache.peek(sample)?.clone();
        let sr = buf.sample_rate as f64;
        Some((daw_core::compile::warp_map(c, *offset_frames, Some(warp_bpm.unwrap_or(self.project.tempo_bpm)), sr), buf))
    }

    /// Write the clip's groove into its notes and clear the groove.
    pub fn commit_groove(&mut self, cref: ClipRef) {
        let grooves = self.project.grooves.clone();
        let amount = self.project.groove_amount;
        self.edit(|p| {
            let Some(c) = DawApp::clip_mut(p, cref) else { return };
            let Some(g) = c.groove.and_then(|id| grooves.iter().find(|g| g.id == id)) else { return };
            if let ClipContent::Midi { notes } = &mut c.content {
                for (k, n) in notes.iter_mut().enumerate() {
                    *n = g.apply(n, amount, k as u64);
                }
                notes.sort_by(|a, b| a.start.total_cmp(&b.start));
            }
            c.groove = None;
        });
    }

    /// Double the loop, copying the notes inside it.
    pub fn duplicate_loop(&mut self, cref: ClipRef) {
        self.edit(|p| {
            let Some(c) = DawApp::clip_mut(p, cref) else { return };
            c.fix_loop();
            let (a, b) = (c.loop_start, c.loop_end);
            let l = b - a;
            if let ClipContent::Midi { notes } = &mut c.content {
                notes.retain(|n| !(n.start >= b - 1e-9 && n.start < b + l));
                let copies: Vec<Note> =
                    notes.iter().filter(|n| n.start >= a - 1e-9 && n.start < b).map(|n| Note { start: n.start + l, ..*n }).collect();
                notes.extend(copies);
                notes.sort_by(|x, y| x.start.total_cmp(&y.start));
            }
            c.loop_end = b + l;
            if matches!(cref, ClipRef::Slot { .. }) {
                c.length = c.loop_end - c.loop_start;
            }
        });
    }

    /// Drop everything outside the loop (or start..end); the region becomes 1.1.1.
    pub fn crop_clip(&mut self, cref: ClipRef) {
        let Some(clip) = self.clip(cref).cloned() else { return };
        let (a, b) = region(&clip);
        let len = b - a;
        // Audio: render the region's frames into a new sample.
        let mut new_audio: Option<(String, f64)> = None;
        if let Some((map, buf)) = self.audio_map_of(&clip) {
            let (f0, f1) = (map.frame_at(a).max(0.0) as usize, (map.frame_at(b).max(0.0) as usize).min(buf.len()));
            if f1 <= f0 {
                return;
            }
            let data: Vec<f32> = (f0..f1).flat_map(|i| [buf.left[i], buf.right[i]]).collect();
            let key = self.store_audio("crop", &data);
            new_audio = Some((key, f0 as f64));
        }
        self.edit(|p| {
            let Some(c) = DawApp::clip_mut(p, cref) else { return };
            match &mut c.content {
                ClipContent::Midi { notes } => {
                    notes.retain(|n| n.start >= a - 1e-9 && n.start < b);
                    notes.iter_mut().for_each(|n| {
                        n.start -= a;
                        n.length = n.length.min(len - n.start);
                    });
                }
                ClipContent::Audio { sample, offset_frames, .. } => {
                    if let Some((key, f0)) = &new_audio {
                        *sample = key.clone();
                        *offset_frames = (*offset_frames - f0).max(0.0);
                        for m in &mut c.audio.markers {
                            m.frame -= f0;
                        }
                        c.audio.transients = None;
                    }
                }
            }
            c.shift_markers(a);
            if let ClipContent::Audio { offset_frames, .. } = &mut c.content
                && c.audio.markers.is_empty()
            {
                *offset_frames = 0.0;
            }
            c.audio.markers.retain(|m| m.beat >= -1e-9 && m.beat <= len + 1e-9);
            c.start = 0.0;
            (c.loop_start, c.loop_end) = (0.0, len);
            c.length = len;
            c.head = 0.0;
            c.full = 0.0;
        });
    }

    /// Reverse an audio clip into a new sample, mirroring markers.
    pub fn reverse_clip(&mut self, cref: ClipRef) {
        let Some(clip) = self.clip(cref).cloned() else { return };
        let Some((map, buf)) = self.audio_map_of(&clip) else { return };
        let total = buf.len() as f64;
        let data: Vec<f32> = (0..buf.len()).rev().flat_map(|i| [buf.left[i], buf.right[i]]).collect();
        let key = self.store_audio("reverse", &data);
        let (w0, w1) = crate::ui::clip::play_window(&clip);
        self.edit(|p| {
            let Some(c) = DawApp::clip_mut(p, cref) else { return };
            if let ClipContent::Audio { sample, offset_frames, .. } = &mut c.content {
                *sample = key;
                if c.audio.markers.is_empty() {
                    *offset_frames = (total - map.frame_at(w0 + w1)).max(0.0);
                } else {
                    for m in &mut c.audio.markers {
                        *m = WarpMarker { beat: w0 + w1 - m.beat, frame: total - m.frame };
                    }
                    c.audio.markers.sort_by(|x, y| x.beat.total_cmp(&y.beat));
                }
                c.audio.transients = None;
            }
        });
    }

    /// Warp markers onto the grid (created at transients if there are none).
    pub fn quantize_warp(&mut self, cref: ClipRef, grid: f64, amount: f64) {
        let Some(clip) = self.clip(cref).cloned() else { return };
        let Some((map, buf)) = self.audio_map_of(&clip) else { return };
        let (w0, w1) = crate::ui::clip::play_window(&clip);
        let tr: Vec<f64> = clip.audio.transients.clone().unwrap_or_else(|| buf.transients.clone());
        self.edit(|p| {
            let Some(c) = DawApp::clip_mut(p, cref) else { return };
            if c.audio.markers.is_empty() {
                c.audio.markers = tr
                    .iter()
                    .map(|&f| WarpMarker { beat: map.beat_at(f), frame: f })
                    .filter(|m| m.beat >= w0 - 1e-9 && m.beat <= w1)
                    .collect();
            }
            let mut last = f64::NEG_INFINITY;
            for m in &mut c.audio.markers {
                let q = (m.beat / grid).round() * grid;
                m.beat += (q - m.beat) * amount;
                // Keep strictly increasing.
                m.beat = m.beat.max(last + 1.0 / 64.0);
                last = m.beat;
            }
        });
    }

    /// Warp the whole sample as an N-bar loop.
    pub fn warp_as(&mut self, cref: ClipRef, bars: f64, _total: f64) {
        let Some(clip) = self.clip(cref).cloned() else { return };
        let Some((_, buf)) = self.audio_map_of(&clip) else { return };
        let beats = bars * self.project.beats_per_bar as f64;
        let total = buf.len() as f64;
        let sr = buf.sample_rate as f64;
        self.edit(|p| {
            let Some(c) = DawApp::clip_mut(p, cref) else { return };
            if let ClipContent::Audio { offset_frames, warp_bpm, .. } = &mut c.content {
                *offset_frames = 0.0;
                *warp_bpm = Some(beats * 60.0 * sr / total);
            }
            c.audio.markers = vec![WarpMarker { beat: 0.0, frame: 0.0 }, WarpMarker { beat: beats, frame: total }];
            c.start = 0.0;
            (c.loop_start, c.loop_end) = (0.0, beats);
            if matches!(cref, ClipRef::Slot { .. }) {
                c.length = beats;
            }
        });
    }

    /// Move the loop brace by `dir` loop lengths.
    pub fn shift_loop(&mut self, cref: ClipRef, dir: f64) {
        self.edit(|p| {
            if let Some(c) = DawApp::clip_mut(p, cref) {
                c.fix_loop();
                let l = c.loop_end - c.loop_start;
                c.loop_start += dir * l;
                c.loop_end += dir * l;
            }
        });
    }

    /// Auto Apply off: put back the notes from before the tool ran.
    pub fn restore_tool_base(&mut self, cref: ClipRef) {
        let Some(base) = self.cv.base.take() else { return };
        let cur = match self.clip(cref) {
            Some(Clip { content: ClipContent::Midi { notes }, id, .. }) if *id == base.clip_id() => notes.clone(),
            _ => return,
        };
        if cur != *base.applied() {
            return;
        }
        let (notes, mpe) = base.into_original();
        if let Some(c) = DawApp::clip_mut(&mut self.project, cref) {
            c.content = ClipContent::Midi { notes };
            c.mpe = mpe;
            self.needs_compile = true;
            self.dirty = true;
        }
        self.note_sel.clear();
    }

    /// Run the current MIDI tool. Auto Apply re-runs from the stored original
    /// so parameter changes don't accumulate; `force` (Apply) always checkpoints.
    pub fn run_tool(&mut self, cref: ClipRef, clip: &Clip, force: bool) {
        let ClipContent::Midi { notes: cur } = &clip.content else { return };
        let gen_ = self.cv.panel == crate::ui::clip::NotesPanel::Generate;
        let stale = force || self.cv.base.as_ref().is_none_or(|b| b.clip_id() != clip.id || b.applied() != cur);
        if stale {
            // Targets: selection → time selection → loop.
            let (ra, rb) = match self.pr_time_sel {
                Some(r) => r,
                None if !self.note_sel.is_empty() && !gen_ => {
                    let s = self.note_sel.iter().filter_map(|&i| cur.get(i)).map(|n| n.start).fold(f64::INFINITY, f64::min);
                    let e = self.note_sel.iter().filter_map(|&i| cur.get(i)).map(|n| n.start + n.length).fold(f64::NEG_INFINITY, f64::max);
                    (s, e)
                }
                None => region(clip),
            };
            let targets: Vec<usize> = if gen_ {
                (0..cur.len()).filter(|&i| cur[i].start >= ra - 1e-9 && cur[i].start < rb).collect()
            } else if !self.note_sel.is_empty() {
                self.note_sel.iter().copied().filter(|&i| i < cur.len()).collect()
            } else {
                (0..cur.len()).filter(|&i| cur[i].start >= ra - 1e-9 && cur[i].start < rb).collect()
            };
            self.begin_gesture();
            self.cv.base = Some(crate::ui::clip::ToolBase::new(clip.id, cur.clone(), clip.mpe.clone(), targets, (ra, rb)));
        }
        let base = self.cv.base.as_ref().expect("set above");
        let (orig, orig_mpe, targets, range) = base.parts();
        let cx = Ctx { range, scale: clip.scale.on.then_some(clip.scale) };
        let mut mpe = orig_mpe.to_vec();
        let input: Vec<Note> = targets.iter().map(|&i| orig[i]).collect();
        let out = if gen_ {
            let i = mt::Generate::ALL.iter().position(|g| *g == self.cv.generate).unwrap_or(0);
            mt::generate(self.cv.generate, &self.cv.g_params[i], &cx)
        } else {
            let i = mt::Transform::ALL.iter().position(|t| *t == self.cv.transform).unwrap_or(0);
            mt::transform(self.cv.transform, &self.cv.t_params[i], &input, &mut mpe, &cx)
        };
        let mut notes: Vec<Note> = orig.iter().enumerate().filter(|(i, _)| !targets.contains(i)).map(|(_, n)| *n).collect();
        let keep = notes.len();
        notes.extend(out.iter().copied());
        // Selection = the tool's output (re-found after sorting).
        let mut idx: Vec<usize> = (0..notes.len()).collect();
        idx.sort_by(|&a, &b| notes[a].start.total_cmp(&notes[b].start).then(notes[a].pitch.cmp(&notes[b].pitch)));
        let sorted: Vec<Note> = idx.iter().map(|&i| notes[i]).collect();
        self.note_sel = idx.iter().enumerate().filter(|(_, i)| **i >= keep).map(|(k, _)| k).collect();
        if let Some(b) = self.cv.base.as_mut() {
            b.set_applied(sorted.clone());
        }
        if let Some(c) = DawApp::clip_mut(&mut self.project, cref) {
            c.content = ClipContent::Midi { notes: sorted };
            c.mpe = mpe;
            c.compact_mpe();
            // compact_mpe may renumber: keep the stored result in sync.
            if let (ClipContent::Midi { notes }, Some(b)) = (&c.content, self.cv.base.as_mut()) {
                b.set_applied(notes.clone());
            }
        }
        self.needs_compile = true;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{ClipRef, DawApp};
    use daw_project::miditools::{Transform, grid_beats};
    use daw_project::{ArrangementClip, Clip, ClipContent, Note, Project, TrackKind};

    fn app() -> DawApp {
        let (audio, engine) = crate::audio::start(&crate::audio::AudioOptions { force_null: true, no_input: true, ..Default::default() });
        let mut a = DawApp::new(engine, audio, Project::default(), None, false, Default::default(), Default::default());
        a.project.tracks.clear();
        a.project.add_track(TrackKind::Midi);
        a.project.add_track(TrackKind::Audio);
        a
    }

    fn n(start: f64, pitch: u8) -> Note {
        Note { start, length: 0.25, pitch, velocity: 100, muted: false, mpe: 0 }
    }

    fn slot(a: &mut DawApp, notes: Vec<Note>) -> ClipRef {
        let id = a.project.alloc_id();
        let t = &mut a.project.tracks[0];
        t.slots.resize(1, None);
        t.slots[0] = Some(Clip { id, length: 4.0, content: ClipContent::Midi { notes }, ..Default::default() });
        ClipRef::Slot { track: 0, scene: 0 }
    }

    fn notes(a: &DawApp, c: ClipRef) -> Vec<Note> {
        match &a.clip(c).unwrap().content {
            ClipContent::Midi { notes } => notes.clone(),
            _ => vec![],
        }
    }

    #[test]
    fn duplicate_loop_copies_notes_and_doubles_length() {
        let mut a = app();
        let c = slot(&mut a, vec![n(1.0, 60)]);
        a.duplicate_loop(c);
        let cl = a.clip(c).unwrap();
        assert_eq!((cl.loop_start, cl.loop_end, cl.length), (0.0, 8.0, 8.0));
        assert_eq!(notes(&a, c).iter().map(|n| n.start).collect::<Vec<_>>(), vec![1.0, 5.0]);
    }

    #[test]
    fn loop_brace_shifts_by_its_length() {
        let mut a = app();
        let c = slot(&mut a, vec![]);
        let cl = DawApp::clip_mut(&mut a.project, c).unwrap();
        (cl.loop_start, cl.loop_end) = (1.0, 3.0);
        a.shift_loop(c, 1.0);
        let cl = a.clip(c).unwrap();
        assert_eq!((cl.loop_start, cl.loop_end), (3.0, 5.0));
        a.shift_loop(c, -2.0);
        let cl = a.clip(c).unwrap();
        assert_eq!((cl.loop_start, cl.loop_end), (-1.0, 1.0));
    }

    #[test]
    fn auto_apply_reruns_from_original_and_restores() {
        let mut a = app();
        let c = slot(&mut a, vec![n(0.1, 60), n(1.3, 62)]);
        a.cv.transform = Transform::Quantize;
        let qi = Transform::ALL.iter().position(|t| *t == Transform::Quantize).unwrap();
        a.cv.t_params[qi][1] = 1.0; // Start
        a.cv.t_params[qi][3] = 100.0; // Amount
        let g = grid_beats(a.cv.t_params[qi][0]);
        let clip = a.clip(c).unwrap().clone();
        a.run_tool(c, &clip, false);
        let first = notes(&a, c);
        for x in &first {
            assert!(((x.start / g).round() * g - x.start).abs() < 1e-9, "{x:?} on grid {g}");
        }
        // Parameter tweak (Amount 50) re-runs from the original, not the result.
        a.cv.t_params[qi][3] = 50.0;
        let clip = a.clip(c).unwrap().clone();
        a.run_tool(c, &clip, false);
        let half = notes(&a, c);
        assert!(g != 0.25 || (half[0].start - 0.05).abs() < 1e-9);
        assert_ne!(half, first);
        a.restore_tool_base(c);
        assert_eq!(notes(&a, c), vec![n(0.1, 60), n(1.3, 62)]);
    }

    #[test]
    fn reverse_and_warp_as_audio() {
        let mut a = app();
        let data: Vec<f32> = (0..4800).flat_map(|i| [i as f32 / 4800.0, 0.0]).collect();
        let key = a.store_audio("test", &data);
        let id = a.project.alloc_id();
        let clip = Clip {
            id,
            length: 1.0,
            looping: false,
            content: ClipContent::Audio { sample: key.clone(), offset_frames: 0.0, warp_bpm: None, gain_db: 0.0 },
            ..Default::default()
        };
        a.project.tracks[1].arrangement.push(ArrangementClip { start: 0.0, clip });
        let c = ClipRef::Arr { track: 1, index: 0 };
        a.reverse_clip(c);
        let ClipContent::Audio { sample, .. } = &a.clip(c).unwrap().content else { panic!() };
        assert_ne!(*sample, key);
        let buf = a.cache.peek(sample).unwrap().clone();
        assert_eq!(buf.len(), 4800);
        assert!((buf.left[0] - 4799.0 / 4800.0).abs() < 1e-6 && buf.left[4799] == 0.0);
        // Undo brings the original back.
        a.undo();
        let ClipContent::Audio { sample, .. } = &a.clip(c).unwrap().content else { panic!() };
        assert_eq!(*sample, key);

        a.warp_as(c, 1.0, 0.0);
        let cl = a.clip(c).unwrap();
        let bpb = a.project.beats_per_bar as f64;
        let sr = a.cache.peek(&key).unwrap().sample_rate as f64;
        let ClipContent::Audio { warp_bpm, .. } = &cl.content else { panic!() };
        assert!((warp_bpm.unwrap() - bpb * 60.0 * sr / 4800.0).abs() < 1e-9);
        assert_eq!(cl.audio.markers.last().map(|m| (m.beat, m.frame)), Some((bpb, 4800.0)));
    }
}
