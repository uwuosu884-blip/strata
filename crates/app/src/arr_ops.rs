//! Arrangement selection state and the editing commands that act on it
//! (Ctrl+L loop, Ctrl+E split, Ctrl+Shift+M insert MIDI clip, Ctrl+G group …).

use crate::app::{ClipRef, DawApp};
use daw_engine::Command;
use daw_project::{Clip, ClipContent, TrackKind};
use std::collections::HashMap;

/// Time selection: `[start, end)` beats over the visible rows from track `t0` to `t1` (t0 <= t1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeSel {
    pub start: f64,
    pub end: f64,
    pub t0: usize,
    pub t1: usize,
}

impl TimeSel {
    pub fn new(a: f64, b: f64, ta: usize, tb: usize) -> Self {
        Self { start: a.min(b), end: a.max(b), t0: ta.min(tb), t1: ta.max(tb) }
    }
    pub fn len(&self) -> f64 {
        self.end - self.start
    }
    pub fn contains(&self, beat: f64, track: usize) -> bool {
        (self.t0..=self.t1).contains(&track) && beat >= self.start && beat < self.end
    }
}

const EPS: f64 = 1e-6;

impl DawApp {
    /// Source frames per beat for audio clips (offsets are in source frames).
    pub fn fpb_fn(&self) -> impl Fn(&Clip) -> f64 + use<> {
        let tempo = self.project.tempo_bpm;
        let sr = self.audio.info.sample_rate as f64;
        let mut rates = HashMap::new();
        for a in self.project.tracks.iter().flat_map(|t| &t.arrangement) {
            if let ClipContent::Audio { sample, .. } = &a.clip.content
                && let Some(b) = self.cache.peek(sample)
            {
                rates.insert(sample.clone(), b.sample_rate as f64);
            }
        }
        move |c: &Clip| match &c.content {
            ClipContent::Audio { warp_bpm, sample, .. } => 60.0 / warp_bpm.unwrap_or(tempo) * rates.get(sample).copied().unwrap_or(sr),
            ClipContent::Midi { .. } => 0.0,
        }
    }

    // ----- track selection ------------------------------------------------------

    /// Selected tracks (multi-selection via Shift+click, else the primary one).
    pub fn selected_tracks(&self) -> Vec<usize> {
        if self.track_sel.is_empty() {
            if self.selected_track < self.project.tracks.len() { vec![self.selected_track] } else { vec![] }
        } else {
            self.track_sel.clone()
        }
    }

    pub fn is_track_selected(&self, t: usize) -> bool {
        if self.track_sel.is_empty() { self.selected_track == t } else { self.track_sel.contains(&t) }
    }

    /// Header click: plain selects one track, Shift/Ctrl toggles it in the selection.
    pub fn click_track(&mut self, t: usize, toggle: bool) {
        if toggle {
            if self.track_sel.is_empty() {
                self.track_sel.push(self.selected_track);
            }
            if let Some(i) = self.track_sel.iter().position(|&x| x == t) {
                if self.track_sel.len() > 1 {
                    self.track_sel.remove(i);
                    if self.selected_track == t {
                        self.selected_track = self.track_sel[0];
                    }
                }
            } else {
                self.track_sel.push(t);
                self.selected_track = t;
            }
            self.track_sel.sort_unstable();
            self.track_sel.dedup();
        } else {
            self.track_sel.clear();
            self.selected_track = t;
        }
    }

    /// Indices invalidated (structure change): drop index-based selections.
    pub fn clear_structure_selection(&mut self) {
        self.track_sel.clear();
        self.time_sel = None;
        self.resync_selected_clip();
    }

    /// Keep selections valid after undo/redo or edits elsewhere.
    pub fn sanitize_selection(&mut self) {
        let n = self.project.tracks.len();
        self.track_sel.retain(|&t| t < n);
        if let Some(ts) = &mut self.time_sel {
            if n == 0 {
                self.time_sel = None;
            } else {
                ts.t0 = ts.t0.min(n - 1);
                ts.t1 = ts.t1.min(n - 1);
            }
        }
        if !self.arr_sel.is_empty() {
            let p = &self.project;
            self.arr_sel.retain(|&id| p.find_arrangement_clip(id).is_some());
        }
    }

    /// Point the detail panel at the first selected arrangement clip (indices shift on edits).
    pub fn resync_selected_clip(&mut self) {
        if let Some(ClipRef::Arr { .. }) = self.selected_clip {
            self.selected_clip = None;
        }
        if let Some((track, index)) = self.arr_sel.first().and_then(|&id| self.project.find_arrangement_clip(id)) {
            self.selected_clip = Some(ClipRef::Arr { track, index });
            self.selected_track = track;
        }
    }

    /// Select one arrangement clip (or toggle it with `toggle`).
    pub fn select_arr_clip(&mut self, id: u64, toggle: bool) {
        if toggle {
            if let Some(i) = self.arr_sel.iter().position(|&x| x == id) {
                self.arr_sel.remove(i);
            } else {
                self.arr_sel.insert(0, id);
            }
        } else if !(self.arr_sel.len() > 1 && self.arr_sel.contains(&id)) {
            self.arr_sel = vec![id];
        }
        self.resync_selected_clip();
    }

    /// Tracks covered by a time selection: visible rows in range; a group row covers its children.
    pub fn time_sel_tracks(&self, ts: &TimeSel) -> Vec<usize> {
        let p = &self.project;
        let mut out = Vec::new();
        for t in ts.t0..=ts.t1.min(p.tracks.len().saturating_sub(1)) {
            if !p.is_visible(t) {
                continue;
            }
            if p.tracks[t].kind == TrackKind::Group {
                out.extend((t + 1..p.subtree_end(t)).filter(|&c| p.tracks[c].kind != TrackKind::Group));
            } else {
                out.push(t);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    // ----- commands -------------------------------------------------------------

    /// Ctrl+L: loop the time selection (or the selected clips) and enable the loop.
    pub fn loop_selection(&mut self) {
        let range = match self.time_sel {
            Some(ts) if ts.len() > EPS => Some((ts.start, ts.end)),
            _ => {
                let p = &self.project;
                self.arr_sel
                    .iter()
                    .filter_map(|&id| p.find_arrangement_clip(id))
                    .map(|(t, i)| {
                        let a = &p.tracks[t].arrangement[i];
                        (a.start, a.start + a.clip.length)
                    })
                    .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
            }
        };
        let Some((s, e)) = range else {
            self.toast("Select a region or clip to loop");
            return;
        };
        self.edit(|p| {
            p.loop_start = s;
            p.loop_end = e;
            p.loop_enabled = true;
        });
        self.send_loop();
    }

    /// Ctrl+E: split at the time selection's edges, else the selected clips (or
    /// clips on the selected tracks) at the playhead.
    pub fn split_selection(&mut self) {
        let fpb = self.fpb_fn();
        if let Some(ts) = self.time_sel
            && ts.len() > EPS
        {
            let tracks = self.time_sel_tracks(&ts);
            let crosses = tracks.iter().any(|&t| {
                self.project.tracks[t].arrangement.iter().any(|a| {
                    let (s, e) = (a.start, a.start + a.clip.length);
                    (s + EPS < ts.start && ts.start < e - EPS) || (s + EPS < ts.end && ts.end < e - EPS)
                })
            });
            if crosses {
                self.arr_sel = self.edit(|p| p.split_range(&tracks, ts.start, ts.end, &fpb));
                self.resync_selected_clip();
                return;
            }
        }
        let at = self.status.beat_position;
        let p = &self.project;
        let mut targets: Vec<(usize, u64)> =
            self.arr_sel.iter().filter_map(|&id| p.find_arrangement_clip(id).map(|(t, _)| (t, id))).collect();
        if targets.is_empty() {
            for t in self.selected_tracks() {
                targets.extend(
                    p.tracks[t].arrangement.iter().filter(|a| a.start < at && at < a.start + a.clip.length).map(|a| (t, a.clip.id)),
                );
            }
        }
        let inside = |p: &daw_project::Project, (t, id): (usize, u64)| {
            p.tracks[t].arrangement.iter().any(|a| a.clip.id == id && a.start + EPS < at && at < a.start + a.clip.length - EPS)
        };
        targets.retain(|&x| inside(p, x));
        if targets.is_empty() {
            self.toast("Nothing to split at the playhead");
            return;
        }
        self.edit(|p| {
            for (t, id) in targets {
                p.split_arrangement_clip(t, id, at, &fpb);
            }
        });
        self.resync_selected_clip();
    }

    /// Ctrl+Shift+M: new MIDI clip over the time selection on each selected MIDI track.
    pub fn insert_midi_clip_in_selection(&mut self) {
        let Some(ts) = self.time_sel.filter(|t| t.len() > EPS) else {
            self.toast("Select a region on a MIDI track first (drag in the arrangement)");
            return;
        };
        let tracks: Vec<usize> =
            self.time_sel_tracks(&ts).into_iter().filter(|&t| self.project.tracks[t].kind == TrackKind::Midi).collect();
        if tracks.is_empty() {
            self.toast("The selection has no MIDI track");
            return;
        }
        if let Some(t) = tracks.iter().find(|&&t| self.project.range_has_clips(t, ts.start, ts.end)) {
            let name = self.project.tracks[*t].name.clone();
            self.toast(format!("“{name}”: the selection already contains clips"));
            return;
        }
        let ids: Vec<u64> = self.edit(|p| tracks.iter().filter_map(|&t| p.insert_midi_clip(t, ts.start, ts.end).ok()).collect());
        self.arr_sel = ids;
        self.resync_selected_clip();
    }

    /// Del in the arrangement: time selection material, else selected clips. False if nothing.
    pub fn delete_arrangement_selection(&mut self) -> bool {
        let fpb = self.fpb_fn();
        if let Some(ts) = self.time_sel.filter(|t| t.len() > EPS) {
            let tracks = self.time_sel_tracks(&ts);
            self.edit(|p| p.delete_range(&tracks, ts.start, ts.end, &fpb));
            self.arr_sel.clear();
            self.resync_selected_clip();
            return true;
        }
        if self.arr_sel.is_empty() {
            return false;
        }
        let ids = std::mem::take(&mut self.arr_sel);
        self.edit(|p| {
            for id in ids {
                if let Some((t, i)) = p.find_arrangement_clip(id) {
                    p.tracks[t].arrangement.remove(i);
                }
            }
        });
        self.resync_selected_clip();
        true
    }

    /// Ctrl+D in the arrangement: duplicate the time selection (moving the
    /// selection onto the copy) or the selected clips. False if nothing selected.
    pub fn duplicate_arrangement_selection(&mut self) -> bool {
        let fpb = self.fpb_fn();
        if let Some(ts) = self.time_sel.filter(|t| t.len() > EPS) {
            let tracks = self.time_sel_tracks(&ts);
            self.arr_sel = self.edit(|p| p.duplicate_range(&tracks, ts.start, ts.end, &fpb));
            self.time_sel = Some(TimeSel { start: ts.end, end: ts.end + ts.len(), ..ts });
            self.resync_selected_clip();
            return true;
        }
        let p = &self.project;
        let clips: Vec<(usize, f64, f64, u64)> = self
            .arr_sel
            .iter()
            .filter_map(|&id| p.find_arrangement_clip(id))
            .map(|(t, i)| {
                let a = &p.tracks[t].arrangement[i];
                (t, a.start, a.start + a.clip.length, a.clip.id)
            })
            .collect();
        if clips.is_empty() {
            return false;
        }
        let (s, e) = clips.iter().fold((f64::MAX, f64::MIN), |acc, c| (acc.0.min(c.1), acc.1.max(c.2)));
        let shift = e - s;
        let ids = self.edit(|p| {
            let mut ids = Vec::new();
            for &(t, _, _, id) in &clips {
                let Some((_, i)) = p.find_arrangement_clip(id) else { continue };
                let mut a = p.tracks[t].arrangement[i].clone();
                a.start += shift;
                a.clip.id = p.alloc_id();
                ids.push(a.clip.id);
                p.clear_arrangement_range(t, a.start, a.start + a.clip.length, &fpb);
                p.tracks[t].arrangement.push(a);
                p.tracks[t].arrangement.sort_by(|x, y| x.start.total_cmp(&y.start));
            }
            ids
        });
        self.arr_sel = ids;
        self.resync_selected_clip();
        true
    }

    /// Split the time selection on its tracks and select the inside pieces
    /// (start of a "drag part of a clip out" gesture). One undo step.
    pub fn split_for_drag(&mut self, ts: TimeSel) -> Vec<u64> {
        let fpb = self.fpb_fn();
        let tracks = self.time_sel_tracks(&ts);
        let ids = self.edit(|p| p.split_range(&tracks, ts.start, ts.end, &fpb));
        self.arr_sel = ids.clone();
        self.resync_selected_clip();
        ids
    }

    /// Apply a clip drag. `checkpoint` = false when the gesture already made its undo step.
    pub fn move_arr_clips(&mut self, moves: &[(u64, usize, f64)], checkpoint: bool) {
        let fpb = self.fpb_fn();
        if checkpoint {
            self.edit(|p| p.move_clips(moves, &fpb));
        } else {
            self.project.move_clips(moves, &fpb);
            self.dirty = true;
            self.needs_compile = true;
        }
        self.resync_selected_clip();
    }

    /// Ctrl+G: group the selected tracks.
    pub fn group_selected_tracks(&mut self) {
        let sel = self.selected_tracks();
        if sel.is_empty() {
            return;
        }
        if let Some(g) = self.edit(|p| p.group_tracks(&sel)) {
            self.clear_structure_selection();
            self.selected_track = g;
        }
    }

    /// Ctrl+Shift+G: dissolve the selected group(s).
    pub fn ungroup_selected(&mut self) {
        let mut groups: Vec<u64> = self
            .selected_tracks()
            .into_iter()
            .filter(|&t| self.project.tracks[t].kind == TrackKind::Group)
            .map(|t| self.project.tracks[t].id)
            .collect();
        if groups.is_empty() {
            self.toast("Select a group track to ungroup");
            return;
        }
        groups.dedup();
        self.edit(|p| {
            for id in groups {
                if let Some(i) = p.track_index(id) {
                    p.ungroup(i);
                }
            }
        });
        self.clear_structure_selection();
    }

    pub fn toggle_fold(&mut self, t: usize) {
        if self.project.tracks.get(t).is_some_and(|x| x.kind == TrackKind::Group) {
            // View state only: no undo step, but saved with the project.
            self.project.tracks[t].folded = !self.project.tracks[t].folded;
            self.dirty = true;
        }
    }

    /// Session: launch a group's slot = launch each child's clip in that scene, stop the others.
    pub fn launch_group_slot(&mut self, g: usize, scene: usize) {
        let end = self.project.subtree_end(g);
        for c in g + 1..end {
            let t = &self.project.tracks[c];
            if t.kind == TrackKind::Group {
                continue;
            }
            let has = t.slots.get(scene).is_some_and(Option::is_some);
            if let Some(rt) = self.rt(c) {
                self.send(if has { Command::LaunchClip { track: rt, scene: scene as u32 } } else { Command::StopTrack { track: rt } });
            }
        }
    }

    /// Stop every track inside a group.
    pub fn stop_group(&mut self, g: usize) {
        for c in g + 1..self.project.subtree_end(g) {
            if let Some(rt) = self.rt(c) {
                self.send(Command::StopTrack { track: rt });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TimeSel;

    #[test]
    fn time_sel_normalizes() {
        let t = TimeSel::new(8.0, 2.0, 3, 1);
        assert_eq!((t.start, t.end, t.t0, t.t1), (2.0, 8.0, 1, 3));
        assert!(t.contains(2.0, 2) && !t.contains(8.0, 2) && !t.contains(4.0, 0));
        assert_eq!(t.len(), 6.0);
    }
}
