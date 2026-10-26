//! Arrangement selection state and the editing commands that act on it
//! (Ctrl+L loop, Ctrl+E split, Ctrl+Shift+M insert MIDI clip, Ctrl+G group …).

use crate::app::{ClipRef, DawApp, DetailMode};
use daw_engine::Command;
use daw_project::{ArrangementClip, Clip, ClipContent, Note, Project, TrackDrop, TrackKind};
use std::collections::HashMap;

/// Lane height of a collapsed track (clip title bars only) and the smallest expanded height.
pub const COLLAPSED_H: f32 = 20.0;
pub const ROW_MIN: f32 = 40.0;
pub const ROW_MAX: f32 = 300.0;

/// How a click changes a selection: plain, Ctrl = toggle one item, Shift = range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelMode {
    Single,
    Toggle,
    Range,
}

impl SelMode {
    pub fn from_mods(m: &eframe::egui::Modifiers) -> Self {
        if m.shift {
            Self::Range
        } else if m.command {
            Self::Toggle
        } else {
            Self::Single
        }
    }
}

/// Snap grid (right-click menu in the arrangement / piano roll). Alt bypasses snapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridMode {
    /// Follows the zoom; -2 = widest … 2 = narrowest (Live's adaptive grid).
    Adaptive(i8),
    /// Fixed step in beats.
    Fixed(f64),
    /// No snapping (lines still follow the zoom).
    Off,
}

impl GridMode {
    pub const FIXED: [(f64, &'static str); 8] =
        [(16.0, "4 Bars"), (8.0, "2 Bars"), (4.0, "1 Bar"), (2.0, "1/2"), (1.0, "1/4"), (0.5, "1/8"), (0.25, "1/16"), (0.125, "1/32")];
    pub const ADAPTIVE: [(i8, &'static str); 5] = [(-2, "Widest"), (-1, "Wide"), (0, "Medium"), (1, "Narrow"), (2, "Narrowest")];

    /// Grid step in beats at `ppb` pixels per beat.
    pub fn step(self, ppb: f64, bpb: f64) -> f64 {
        match self {
            Self::Fixed(g) => g,
            Self::Adaptive(level) => adaptive_step(ppb, bpb, 12.0 * 2f64.powi(-level as i32)),
            Self::Off => adaptive_step(ppb, bpb, 12.0),
        }
    }

    pub fn snaps(self) -> bool {
        self != Self::Off
    }
}

/// Smallest musical step that is at least `min_px` wide.
pub fn adaptive_step(ppb: f64, bpb: f64, min_px: f64) -> f64 {
    for g in [0.0625, 0.125, 0.25, 0.5, 1.0] {
        if g * ppb >= min_px {
            return g;
        }
    }
    let mut g = bpb.max(1.0);
    while g * ppb < min_px && g < 4096.0 {
        g *= 2.0;
    }
    g
}

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
        if self.sel_hidden() {
            vec![]
        } else if self.track_sel.is_empty() {
            if self.selected_track < self.project.tracks.len() { vec![self.selected_track] } else { vec![] }
        } else {
            self.track_sel.clone()
        }
    }

    fn sel_hidden(&self) -> bool {
        self.track_sel.is_empty() && self.hide_sel == Some(self.selected_track)
    }

    /// Click on empty space: nothing selected (clips, time, track headers).
    pub fn deselect_all(&mut self) {
        self.time_sel = None;
        self.arr_sel.clear();
        self.track_sel.clear();
        self.hide_sel = Some(self.selected_track);
        self.touched_track = None;
        self.resync_selected_clip();
    }

    pub fn is_track_selected(&self, t: usize) -> bool {
        if self.sel_hidden() {
            false
        } else if self.track_sel.is_empty() {
            self.selected_track == t
        } else {
            self.track_sel.contains(&t)
        }
    }

    /// Header click: explicit track selection (Del deletes these). Shift selects
    /// every visible track between the anchor and `t`, Ctrl toggles `t`.
    /// The detail panel switches to the track's devices.
    pub fn select_track_header(&mut self, t: usize, mode: SelMode) {
        self.hide_sel = None;
        match mode {
            SelMode::Range => {
                let anchor = self.track_anchor.filter(|&a| a < self.project.tracks.len()).unwrap_or(self.selected_track);
                let rows = self.project.visible_tracks();
                let (pa, pt) = (rows.iter().position(|&r| r == anchor), rows.iter().position(|&r| r == t));
                self.track_sel = match (pa, pt) {
                    (Some(a), Some(b)) => rows[a.min(b)..=a.max(b)].to_vec(),
                    _ => vec![t],
                };
                self.selected_track = t;
                if self.track_anchor.is_none() {
                    self.track_anchor = Some(anchor);
                }
            }
            SelMode::Toggle => {
                self.click_track(t, true);
                self.track_anchor = Some(t);
            }
            SelMode::Single => {
                self.click_track(t, false);
                self.track_anchor = Some(t);
            }
        }
        if self.track_sel.is_empty() {
            self.track_sel.push(t);
        }
        self.touched_track = Some(t);
        self.detail_mode = DetailMode::Device;
        self.focus_detail = false;
    }

    /// Header click: plain selects one track, Shift/Ctrl toggles it in the selection.
    pub fn click_track(&mut self, t: usize, toggle: bool) {
        self.touched_track = Some(t);
        self.hide_sel = None;
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
        self.touched_track = None;
        self.track_anchor = None;
        self.time_sel = None;
        self.resync_selected_clip();
    }

    /// Keep selections valid after undo/redo or edits elsewhere.
    pub fn sanitize_selection(&mut self) {
        let n = self.project.tracks.len();
        self.track_sel.retain(|&t| t < n);
        self.touched_track = self.touched_track.filter(|&t| t < n);
        self.track_anchor = self.track_anchor.filter(|&t| t < n);
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
            self.detail_mode = DetailMode::Clip;
        }
    }

    /// Select one arrangement clip (or toggle it with `toggle`).
    pub fn select_arr_clip(&mut self, id: u64, toggle: bool) {
        self.select_arr_clip_mode(id, if toggle { SelMode::Toggle } else { SelMode::Single });
    }

    /// Clip click: plain selects it (keeps a multi-selection it belongs to, for
    /// dragging), Ctrl toggles it, Shift selects every clip inside the rectangle
    /// spanned by the anchor clip and this one (tracks × time, Live).
    pub fn select_arr_clip_mode(&mut self, id: u64, mode: SelMode) {
        let anchor = self.clip_anchor.or(self.arr_sel.first().copied()).filter(|&a| a != id);
        match (mode, anchor.and_then(|a| self.project.find_arrangement_clip(a))) {
            (SelMode::Range, Some((ta, ia))) if self.project.find_arrangement_clip(id).is_some() => {
                let (tb, ib) = self.project.find_arrangement_clip(id).expect("checked");
                let p = &self.project;
                let (a, b) = (&p.tracks[ta].arrangement[ia], &p.tracks[tb].arrangement[ib]);
                let s = a.start.min(b.start);
                let e = (a.start + a.clip.length).max(b.start + b.clip.length);
                let ts = TimeSel::new(s, e, ta, tb);
                let mut sel = vec![id];
                for t in self.time_sel_tracks(&ts) {
                    for c in &p.tracks[t].arrangement {
                        if c.start >= s - EPS && c.start + c.clip.length <= e + EPS && c.clip.id != id {
                            sel.push(c.clip.id);
                        }
                    }
                }
                self.arr_sel = sel;
                if self.clip_anchor.is_none() {
                    self.clip_anchor = anchor;
                }
            }
            (SelMode::Toggle, _) => {
                if let Some(i) = self.arr_sel.iter().position(|&x| x == id) {
                    self.arr_sel.remove(i);
                } else {
                    self.arr_sel.insert(0, id);
                }
                self.clip_anchor = Some(id);
            }
            _ => {
                if !(self.arr_sel.len() > 1 && self.arr_sel.contains(&id)) {
                    self.arr_sel = vec![id];
                }
                self.clip_anchor = Some(id);
            }
        }
        if let Some((t, _)) = self.project.find_arrangement_clip(id) {
            self.touched_track = Some(t);
        }
        self.resync_selected_clip();
    }

    /// Clicking a clip moves the start point (and the playhead) to its start.
    pub fn playhead_to_clip(&mut self, id: u64) {
        if let Some((t, i)) = self.project.find_arrangement_clip(id) {
            let s = self.project.tracks[t].arrangement[i].start;
            if self.status.playing {
                // While playing only the start point for the next play moves (no jump).
                self.play_start = s;
            } else {
                self.set_playhead(s);
            }
        }
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
        let p = &self.project;
        let same = range.is_none_or(|(s, e)| (s - p.loop_start).abs() < EPS && (e - p.loop_end).abs() < EPS);
        if p.loop_enabled && same {
            // Second Ctrl+L on the same region (or with nothing selected): loop off.
            self.edit(|p| p.loop_enabled = false);
            self.send_loop();
            return;
        }
        // Nothing selected: re-apply the previous loop.
        let Some((s, e)) = range.or((p.loop_end - p.loop_start > EPS).then_some((p.loop_start, p.loop_end))) else {
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

impl DawApp {
    // ----- track heights / collapse -------------------------------------------------

    /// Arrangement lane height of track `t`.
    pub fn track_height(&self, t: usize) -> f32 {
        match self.project.tracks.get(t) {
            Some(tr) if tr.collapsed => COLLAPSED_H,
            Some(tr) if tr.height > 0.0 => tr.height.clamp(ROW_MIN, ROW_MAX),
            _ => self.arr_row_h,
        }
    }

    /// Alt+wheel: scale the track touched last (with the rest of the header
    /// selection when it is part of it), or all tracks when none is touched.
    /// Zooming out past the minimum collapses lanes to their clip title bars;
    /// zooming in expands them again.
    pub fn zoom_track_heights(&mut self, factor: f32) {
        let all = self.touched_track.is_none_or(|t| t >= self.project.tracks.len());
        let targets: Vec<usize> = match self.touched_track {
            _ if all => (0..self.project.tracks.len()).collect(),
            Some(t) if self.track_sel.len() > 1 && self.track_sel.contains(&t) => self.track_sel.clone(),
            Some(t) => vec![t],
            None => Vec::new(),
        };
        if all {
            self.arr_row_h = (self.arr_row_h * factor).clamp(ROW_MIN, ROW_MAX);
        }
        for t in targets {
            let h = self.track_height(t);
            let Some(tr) = self.project.tracks.get_mut(t) else { continue };
            if tr.collapsed {
                if factor > 1.0 {
                    tr.collapsed = false;
                    tr.height = ROW_MIN;
                }
                continue;
            }
            let nh = h * factor;
            if nh < ROW_MIN {
                tr.collapsed = true;
                tr.height = ROW_MIN;
            } else {
                tr.height = nh.min(ROW_MAX);
            }
        }
        self.dirty = true;
    }

    /// Header ⊳ button: collapse to clip title bars / expand back.
    pub fn toggle_collapse(&mut self, t: usize) {
        if let Some(tr) = self.project.tracks.get_mut(t) {
            tr.collapsed = !tr.collapsed;
            self.dirty = true;
        }
    }

    // ----- tracks ---------------------------------------------------------------------

    /// Del with tracks selected in the headers.
    pub fn delete_selected_tracks(&mut self) -> bool {
        if self.track_sel.is_empty() {
            return false;
        }
        let ids: Vec<u64> = self.track_sel.iter().filter_map(|&t| self.project.tracks.get(t).map(|x| x.id)).collect();
        self.edit(|p| {
            for id in ids {
                if let Some(i) = p.track_index(id) {
                    p.remove_track(i); // children of a deleted group go with it
                }
            }
        });
        self.selected_clip = None;
        self.arr_sel.clear();
        self.clear_structure_selection();
        self.clamp_selection();
        true
    }

    /// Header drag & drop.
    pub fn drop_tracks(&mut self, ids: &[u64], to: TrackDrop) {
        let mut moved = self.project.clone();
        let Some(mut idx) = moved.move_tracks(ids, to) else { return };
        if moved == self.project {
            return; // dropped where it already was: no undo step
        }
        self.edit(|p| *p = moved);
        self.clear_structure_selection();
        idx.sort_unstable();
        self.selected_track = idx.first().copied().unwrap_or(0);
        self.touched_track = idx.first().copied();
        self.track_anchor = self.touched_track;
        self.track_sel = idx;
    }

    /// Ctrl+R / double-click: start renaming track `t`.
    pub fn start_rename(&mut self, t: usize) {
        if let Some(tr) = self.project.tracks.get(t) {
            self.renaming =
                Some(crate::app::Rename { id: tr.id, text: self.project.track_label(t), fresh: true, clip: false, in_detail: false });
        }
    }

    /// Ctrl+R on a selected clip / double-click its title bar: rename clip `id`
    /// (arrangement or session), in place or in the detail panel's name bar.
    pub fn start_rename_clip(&mut self, id: u64, in_detail: bool) {
        if let Some(c) = clip_by_id(&mut self.project, id) {
            self.renaming = Some(crate::app::Rename { id, text: c.name.clone(), fresh: true, clip: true, in_detail });
            if in_detail {
                // The editor lives in the clip view's name bar.
                self.detail_mode = crate::app::DetailMode::Clip;
            }
        }
    }

    /// Ctrl+R: the selected clip (arrangement selection / selected session
    /// clip), else the selected track.
    pub fn rename_selection(&mut self) {
        use crate::app::View;
        match self.view {
            View::Arrangement if !self.arr_sel.is_empty() => {
                let in_detail = self.focus_detail && self.detail_mode == crate::app::DetailMode::Clip;
                self.start_rename_clip(self.arr_sel[0], in_detail)
            }
            View::Session if matches!(self.selected_clip, Some(ClipRef::Slot { .. })) => {
                if let Some(id) = self.selected_clip.and_then(|c| self.clip(c)).map(|c| c.id) {
                    self.start_rename_clip(id, true);
                }
            }
            _ => self.start_rename(self.selected_track),
        }
    }

    /// Finish a rename. Leaving the shown text unchanged keeps the name (and its auto number).
    pub fn commit_rename(&mut self) {
        let Some(r) = self.renaming.take() else { return };
        let text = r.text.trim().to_string();
        if r.clip {
            let same = clip_by_id(&mut self.project, r.id).is_none_or(|c| c.name == text);
            if !text.is_empty() && !same {
                self.edit(|p| {
                    if let Some(c) = clip_by_id(p, r.id) {
                        c.name = text;
                    }
                });
            }
            return;
        }
        let Some(t) = self.project.track_index(r.id) else { return };
        if text.is_empty() || text == self.project.track_label(t) {
            return;
        }
        self.edit(|p| p.tracks[t].name = text);
    }

    // ----- clips ----------------------------------------------------------------------

    /// Sample length (frames) of an audio clip's source, if loaded.
    pub fn clip_total_frames(&self, c: &Clip) -> Option<f64> {
        match &c.content {
            ClipContent::Audio { sample, .. } => self.cache.peek(sample).map(|b| b.len() as f64),
            _ => None,
        }
    }

    /// 0: (de)activate the selected notes (piano roll focus) or clips.
    pub fn toggle_disable(&mut self) {
        if self.focus_detail
            && !self.note_sel.is_empty()
            && let Some(c) = self.selected_clip
        {
            let sel = self.note_sel.clone();
            self.edit(|p| {
                if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(p, c) {
                    let mute = sel.iter().any(|&i| notes.get(i).is_some_and(|n| !n.muted));
                    for &i in &sel {
                        if let Some(n) = notes.get_mut(i) {
                            n.muted = mute;
                        }
                    }
                }
            });
            return;
        }
        let ids: Vec<u64> = self.arr_sel.clone();
        if !ids.is_empty() {
            let p = &self.project;
            let mute = ids.iter().filter_map(|&id| p.find_arrangement_clip(id)).any(|(t, i)| !p.tracks[t].arrangement[i].clip.muted);
            self.edit(|p| {
                for &id in &ids {
                    if let Some((t, i)) = p.find_arrangement_clip(id) {
                        p.tracks[t].arrangement[i].clip.muted = mute;
                    }
                }
            });
        } else if let Some(c) = self.selected_clip {
            self.edit(|p| {
                if let Some(clip) = DawApp::clip_mut(p, c) {
                    clip.muted = !clip.muted;
                }
            });
        }
    }

    /// Ctrl+D in the piano roll: copy the selected notes together with the
    /// rubber-band time range (if any) right after that range. Without a range
    /// the notes' span is rounded up to the grid. The range moves along so
    /// repeated Ctrl+D keeps appending.
    pub fn duplicate_selected_notes(&mut self) -> bool {
        let Some(c) = self.selected_clip else { return false };
        if !self.focus_detail || (self.note_sel.is_empty() && self.pr_time_sel.is_none()) {
            return false;
        }
        let Some(Clip { content: ClipContent::Midi { notes }, .. }) = self.clip(c) else { return false };
        let picked: Vec<Note> = self.note_sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
        let (mut s, mut e) = picked.iter().fold((f64::MAX, f64::MIN), |(s, e), n| (s.min(n.start), e.max(n.start + n.length)));
        if let Some((a, b)) = self.pr_time_sel {
            s = s.min(a);
            e = e.max(b);
        }
        if e - s <= EPS {
            return false;
        }
        let g = self.pr_grid_now.max(1.0 / 64.0);
        let shift = if self.pr_time_sel.is_some() { e - s } else { (((e - s) / g) - 1e-9).ceil().max(1.0) * g };
        let copies: Vec<Note> = picked.iter().map(|n| Note { start: n.start + shift, ..*n }).collect();
        let end = (e + shift).max(copies.iter().map(|n| n.start + n.length).fold(0.0, f64::max));
        let sel = self.edit(|p| {
            let Some(clip) = DawApp::clip_mut(p, c) else { return Vec::new() };
            if end > clip.length + EPS {
                clip.length = (end / g).ceil() * g;
                clip.full = 0.0;
            }
            let ClipContent::Midi { notes } = &mut clip.content else { return Vec::new() };
            notes.extend(copies.iter().copied());
            notes.sort_by(|a, b| a.start.total_cmp(&b.start));
            copies.iter().filter_map(|c| notes.iter().position(|n| n == c)).collect()
        });
        self.note_sel = sel;
        if self.pr_time_sel.is_some() {
            self.pr_time_sel = Some((s + shift, e + shift));
        }
        true
    }

    /// Apply `f` to the selected notes of the clip in the piano roll (one undo step).
    fn edit_selected_notes(&mut self, f: impl Fn(&mut [Note], &[usize])) -> bool {
        let Some(c) = self.selected_clip else { return false };
        if self.note_sel.is_empty() || !matches!(self.clip(c).map(|x| &x.content), Some(ClipContent::Midi { .. })) {
            return false;
        }
        let sel = self.note_sel.clone();
        let new_sel = self.edit(|p| {
            let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(p, c) else { return Vec::new() };
            let idx: Vec<usize> = sel.iter().copied().filter(|&i| i < notes.len()).collect();
            f(notes, &idx);
            // Re-sort, keeping the selection on the same notes.
            let picked: Vec<Note> = idx.iter().map(|&i| notes[i]).collect();
            notes.sort_by(|a, b| a.start.total_cmp(&b.start));
            let mut out: Vec<usize> = Vec::new();
            for n in &picked {
                if let Some(i) = (0..notes.len()).find(|i| notes[*i] == *n && !out.contains(i)) {
                    out.push(i);
                }
            }
            out
        });
        self.note_sel = new_sel;
        true
    }

    /// ±12 buttons / Shift+Up/Down (octave), Up/Down (semitone): transpose the selected notes.
    pub fn transpose_selected_notes(&mut self, semis: i32) -> bool {
        self.edit_selected_notes(|notes, idx| {
            for &i in idx {
                notes[i].pitch = (notes[i].pitch as i32 + semis).clamp(0, 127) as u8;
            }
        })
    }

    /// ×2 / ÷2: stretch the selected notes (positions from the first selected
    /// note, and lengths) by `factor`.
    pub fn scale_selected_notes(&mut self, factor: f64) -> bool {
        self.edit_selected_notes(|notes, idx| {
            let s0 = idx.iter().map(|&i| notes[i].start).fold(f64::MAX, f64::min);
            for &i in idx {
                let n = &mut notes[i];
                n.start = s0 + (n.start - s0) * factor;
                n.length = (n.length * factor).max(1.0 / 128.0);
            }
        })
    }

    /// Ctrl+A in the piano roll.
    pub fn select_all_notes(&mut self) -> bool {
        let Some(c) = self.selected_clip else { return false };
        let Some(Clip { content: ClipContent::Midi { notes }, .. }) = self.clip(c) else { return false };
        self.note_sel = (0..notes.len()).collect();
        true
    }

    /// Ctrl+J: merge the selected clips (or everything in the time selection) per
    /// track into one clip. Audio is rendered into a new sample.
    pub fn consolidate_selection(&mut self) {
        let fpb = self.fpb_fn();
        let mut p = self.project.clone();
        // (track, clip ids, start, end)
        let mut jobs: Vec<(usize, Vec<u64>, f64, f64)> = Vec::new();
        if let Some(ts) = self.time_sel.filter(|t| t.len() > EPS) {
            for t in self.time_sel_tracks(&ts) {
                let ids = p.split_range(&[t], ts.start, ts.end, &fpb);
                if !ids.is_empty() {
                    jobs.push((t, ids, ts.start, ts.end));
                }
            }
        } else {
            let mut by_track: HashMap<usize, (Vec<u64>, f64, f64)> = HashMap::new();
            for &id in &self.arr_sel {
                if let Some((t, i)) = p.find_arrangement_clip(id) {
                    let a = &p.tracks[t].arrangement[i];
                    let e = by_track.entry(t).or_insert((Vec::new(), f64::MAX, f64::MIN));
                    e.0.push(id);
                    e.1 = e.1.min(a.start);
                    e.2 = e.2.max(a.start + a.clip.length);
                }
            }
            jobs = by_track.into_iter().map(|(t, (ids, s, e))| (t, ids, s, e)).collect();
            jobs.sort_by_key(|j| j.0);
        }
        jobs.retain(|j| j.1.len() > 1 || self.time_sel.is_some() || p.tracks[j.0].kind == TrackKind::Audio);
        if jobs.is_empty() {
            self.toast("Select two or more clips (or a region) to consolidate");
            return;
        }
        let mut new_ids = Vec::new();
        for (t, ids, s, e) in jobs {
            match p.tracks[t].kind {
                TrackKind::Midi => new_ids.extend(p.consolidate_midi(t, &ids, s, e)),
                TrackKind::Audio => match self.render_audio_range(&p, t, &ids, s, e) {
                    Ok(clip) => {
                        let id = p.alloc_id();
                        let tr = &mut p.tracks[t];
                        tr.arrangement.retain(|a| !ids.contains(&a.clip.id));
                        tr.arrangement.push(ArrangementClip { start: s, clip: Clip { id, ..clip } });
                        tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
                        new_ids.push(id);
                    }
                    Err(e) => self.toast(format!("Consolidate failed: {e}")),
                },
                TrackKind::Group => {}
            }
        }
        if new_ids.is_empty() {
            return;
        }
        self.edit(|proj| *proj = p);
        self.arr_sel = new_ids;
        self.time_sel = None;
        self.resync_selected_clip();
    }

    /// Render audio clips `ids` of track `t` over `[s, e)` (clip gain, no track
    /// mixer) into a new recording-style sample; returns the replacement clip (id 0).
    fn render_audio_range(&mut self, p: &Project, t: usize, ids: &[u64], s: f64, e: f64) -> Result<Clip, String> {
        let src = &p.tracks[t];
        let clips: Vec<ArrangementClip> = src
            .arrangement
            .iter()
            .filter(|a| ids.contains(&a.clip.id) && !a.clip.muted)
            .map(|a| ArrangementClip { start: a.start - s, clip: a.clip.clone() })
            .collect();
        let first = src.arrangement.iter().find(|a| ids.contains(&a.clip.id)).map(|a| a.clip.clone()).ok_or("no clips")?;
        let mut mini = Project { tempo_bpm: p.tempo_bpm, beats_per_bar: p.beats_per_bar, ..Default::default() };
        let mt = mini.add_track(TrackKind::Audio);
        mini.tracks[mt].arrangement = clips;
        mini.tracks[mt].gain_db = 0.0; // unity: only clip gain is baked in
        mini.tracks[mt].pan = 0.0;
        let sr = self.audio.info.sample_rate;
        let len = e - s;
        let opts = daw_core::ExportOptions {
            sample_rate: sr,
            bars: Some(len / p.beats_per_bar.max(1) as f64),
            tail_seconds: 0.0,
            ..Default::default()
        };
        let r = daw_core::export::render_with_cache(&mini, &mut self.cache, &opts)?;
        let frames = (len * 60.0 / p.tempo_bpm * sr as f64).round() as usize;
        let mut data = r.data;
        data.resize(frames * 2, 0.0);
        let key = self.store_audio("consolidated", &data);
        Ok(Clip {
            id: 0,
            name: first.name.clone(),
            color: first.color,
            length: len,
            looping: false,
            content: ClipContent::Audio { sample: key, offset_frames: 0.0, warp_bpm: Some(p.tempo_bpm), gain_db: 0.0 },
            ..Default::default()
        })
    }
}

/// Arrangement or session clip by id.
pub fn clip_by_id(p: &mut Project, id: u64) -> Option<&mut Clip> {
    p.tracks.iter_mut().find_map(|t| t.arrangement.iter_mut().map(|a| &mut a.clip).chain(t.slots.iter_mut().flatten()).find(|c| c.id == id))
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

    #[test]
    fn adaptive_grid_follows_zoom_and_level() {
        use super::{GridMode, adaptive_step};
        assert_eq!(adaptive_step(100.0, 4.0, 12.0), 0.125);
        assert_eq!(adaptive_step(30.0, 4.0, 12.0), 0.5);
        assert_eq!(adaptive_step(5.0, 4.0, 12.0), 4.0);
        assert_eq!(adaptive_step(1.0, 4.0, 12.0), 16.0);
        assert_eq!(GridMode::Adaptive(-2).step(100.0, 4.0), 0.5);
        assert_eq!(GridMode::Fixed(0.25).step(1.0, 4.0), 0.25);
        assert!(!GridMode::Off.snaps());
    }
}

#[cfg(test)]
mod app_tests {
    use crate::app::{ClipRef, DawApp};
    use daw_project::{ArrangementClip, Clip, ClipContent, Note, Project, TrackKind};

    fn app() -> DawApp {
        let (audio, engine) = crate::audio::start(&crate::audio::AudioOptions { force_null: true, no_input: true, ..Default::default() });
        DawApp::new(engine, audio, Project::default(), None, false, Default::default(), Default::default())
    }

    fn midi_clip(p: &mut Project, t: usize, start: f64, len: f64, pitch: u8) -> u64 {
        let id = p.alloc_id();
        let notes = vec![Note { start: 0.0, length: 0.5, pitch, velocity: 100, muted: false }];
        p.tracks[t]
            .arrangement
            .push(ArrangementClip { start, clip: Clip { id, length: len, content: ClipContent::Midi { notes }, ..Default::default() } });
        p.tracks[t].arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        id
    }

    fn tracks(n: usize) -> DawApp {
        let mut a = app();
        a.project.tracks.clear();
        for _ in 0..n {
            a.project.add_track(TrackKind::Midi);
        }
        a
    }

    #[test]
    fn shift_click_selects_track_range_ctrl_toggles() {
        use super::SelMode::*;
        let mut a = tracks(4);
        a.select_track_header(0, Single);
        a.select_track_header(2, Range);
        assert_eq!(a.track_sel, vec![0, 1, 2]);
        // Range again from the same anchor.
        a.select_track_header(1, Range);
        assert_eq!(a.track_sel, vec![0, 1]);
        a.select_track_header(3, Toggle);
        a.select_track_header(0, Toggle);
        let mut s = a.track_sel.clone();
        s.sort_unstable();
        assert_eq!(s, vec![1, 3]);
        assert_eq!(a.touched_track, Some(0));
    }

    #[test]
    fn shift_click_selects_clip_rectangle_live_style() {
        use super::SelMode::*;
        let mut a = tracks(3);
        let ca = midi_clip(&mut a.project, 0, 0.0, 4.0, 60);
        let cb = midi_clip(&mut a.project, 1, 4.0, 4.0, 60);
        let cc = midi_clip(&mut a.project, 2, 8.0, 4.0, 60);
        let cd = midi_clip(&mut a.project, 1, 20.0, 4.0, 60);
        a.select_arr_clip_mode(ca, Single);
        a.select_arr_clip_mode(cc, Range);
        let mut s = a.arr_sel.clone();
        s.sort_unstable();
        assert_eq!(s, vec![ca, cb, cc]);
        a.select_arr_clip_mode(cd, Toggle);
        a.select_arr_clip_mode(cb, Toggle);
        let mut s = a.arr_sel.clone();
        s.sort_unstable();
        assert_eq!(s, vec![ca, cc, cd]);
        // Clicking a clip moves the start point to its start (nothing plays).
        a.playhead_to_clip(cc);
        assert_eq!(a.play_start, 8.0);
        assert!(!a.status.playing);
    }

    #[test]
    fn arming_several_tracks_keeps_all_armed() {
        let mut a = tracks(3);
        a.click_arm(0, true);
        a.click_arm(2, true);
        assert!(a.project.tracks[0].arm && a.project.tracks[2].arm && !a.project.tracks[1].arm);
    }

    #[test]
    fn ctrl_l_without_selection_reapplies_the_old_loop() {
        let mut a = tracks(1);
        a.time_sel = Some(crate::arr_ops::TimeSel::new(4.0, 12.0, 0, 0));
        a.loop_selection();
        assert!(a.project.loop_enabled && a.project.loop_start == 4.0 && a.project.loop_end == 12.0);
        a.loop_selection(); // same region: off
        assert!(!a.project.loop_enabled);
        a.time_sel = None;
        a.loop_selection();
        assert!(a.project.loop_enabled && a.project.loop_start == 4.0 && a.project.loop_end == 12.0, "old loop back");
        a.loop_selection();
        assert!(!a.project.loop_enabled);
    }

    #[test]
    fn arm_click_applies_to_the_selection_and_ctrl_is_independent() {
        let mut a = tracks(4);
        a.select_track_header(0, crate::arr_ops::SelMode::Single);
        a.select_track_header(2, crate::arr_ops::SelMode::Range);
        a.click_arm(1, false);
        assert_eq!(a.project.tracks.iter().map(|t| t.arm).collect::<Vec<_>>(), vec![true, true, true, false], "all selected");
        a.click_arm(0, false);
        assert!(a.project.tracks[..3].iter().all(|t| !t.arm), "toggles all off");
        a.click_arm(1, true);
        assert_eq!(a.project.tracks.iter().map(|t| t.arm).collect::<Vec<_>>(), vec![false, true, false, false], "Ctrl: just one");
        a.click_arm(3, false);
        assert!(a.project.tracks[3].arm && !a.project.tracks[0].arm, "unselected track: just itself");
        // No Input: can't be armed.
        a.project.tracks[3].midi_in = daw_project::MidiIn::None;
        a.project.tracks[3].arm = false;
        a.click_arm(3, false);
        assert!(!a.project.tracks[3].arm);
    }

    #[test]
    fn monitor_and_routing_apply_to_selected_tracks() {
        let mut a = tracks(3);
        a.select_track_header(0, crate::arr_ops::SelMode::Single);
        a.select_track_header(1, crate::arr_ops::SelMode::Range);
        a.set_monitor(0, Some(daw_project::Monitor::In));
        assert_eq!(a.project.tracks[1].monitor_mode(), daw_project::Monitor::In);
        assert_eq!(a.project.tracks[2].monitor_mode(), daw_project::Monitor::Auto, "unselected keeps the MIDI default");
        a.set_monitor(1, None);
        assert_eq!(a.project.tracks[0].monitor_mode(), daw_project::Monitor::Auto, "return to default");
        a.set_routing(0, |t| t.midi_channel = 5);
        assert!(a.project.tracks[0].midi_channel == 5 && a.project.tracks[1].midi_channel == 5 && a.project.tracks[2].midi_channel == 0);
    }

    fn note_app() -> (DawApp, ClipRef) {
        let mut a = tracks(1);
        let id = midi_clip(&mut a.project, 0, 0.0, 4.0, 60);
        if let Some(ClipContent::Midi { notes }) = super::clip_by_id(&mut a.project, id).map(|c| &mut c.content) {
            notes.push(Note { start: 1.0, length: 0.5, pitch: 64, velocity: 90, muted: false });
        }
        let c = ClipRef::Arr { track: 0, index: 0 };
        a.selected_clip = Some(c);
        a.focus_detail = true;
        a.pr_grid_now = 0.25;
        (a, c)
    }

    fn notes_of(a: &DawApp, c: ClipRef) -> Vec<(f64, u8, f64)> {
        match &a.clip(c).unwrap().content {
            ClipContent::Midi { notes } => notes.iter().map(|n| (n.start, n.pitch, n.length)).collect(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn ctrl_d_duplicates_the_whole_time_region() {
        let (mut a, c) = note_app();
        // Region 0..3 with only the first note in it: the copy lands at 3, not right after the note.
        a.note_sel = vec![0];
        a.pr_time_sel = Some((0.0, 3.0));
        assert!(a.duplicate_selected_notes());
        assert_eq!(notes_of(&a, c), vec![(0.0, 60, 0.5), (1.0, 64, 0.5), (3.0, 60, 0.5)]);
        assert_eq!(a.pr_time_sel, Some((3.0, 6.0)));
        assert!(a.clip(c).unwrap().length >= 6.0);
    }

    #[test]
    fn octave_and_stretch_only_touch_selected_notes() {
        let (mut a, c) = note_app();
        a.note_sel = vec![1];
        assert!(a.transpose_selected_notes(12));
        assert!(a.scale_selected_notes(2.0));
        assert_eq!(notes_of(&a, c), vec![(0.0, 60, 0.5), (1.0, 76, 1.0)]);
        assert!(a.select_all_notes());
        assert_eq!(a.note_sel.len(), 2);
        assert!(a.transpose_selected_notes(-12));
        assert_eq!(notes_of(&a, c), vec![(0.0, 48, 0.5), (1.0, 64, 1.0)]);
    }

    #[test]
    fn track_height_zoom_targets_selection_and_collapses() {
        let mut a = app();
        a.project.add_track(TrackKind::Midi);
        a.project.add_track(TrackKind::Midi);
        let h0 = a.track_height(0);
        // Selected track only.
        a.touched_track = Some(1);
        a.zoom_track_heights(2.0);
        assert_eq!(a.track_height(0), h0);
        assert!(a.track_height(1) > h0);
        // Nothing touched: all tracks; zooming far out collapses.
        a.touched_track = None;
        for _ in 0..20 {
            a.zoom_track_heights(0.5);
        }
        assert!(a.project.tracks.iter().all(|t| t.collapsed));
        a.zoom_track_heights(1.2);
        assert!(a.project.tracks.iter().all(|t| !t.collapsed));
        a.toggle_collapse(0);
        assert!(a.project.tracks[0].collapsed);
    }

    #[test]
    fn delete_tracks_and_rename() {
        let mut a = app();
        for _ in 0..3 {
            a.project.add_track(TrackKind::Audio);
        }
        assert_eq!(a.project.track_label(2), "3 Audio");
        a.start_rename(2);
        a.renaming.as_mut().unwrap().text = "Bass".into();
        a.commit_rename();
        assert_eq!(a.project.track_label(2), "Bass");
        a.track_sel = vec![0];
        assert!(a.delete_selected_tracks());
        assert_eq!(a.project.tracks.len(), 2);
        // Others renumber, the renamed one keeps its name.
        assert_eq!(a.project.track_label(0), "1 Audio");
        assert_eq!(a.project.track_label(1), "Bass");
        a.track_sel.clear();
        assert!(!a.delete_selected_tracks());
    }

    #[test]
    fn disable_clips_and_notes() {
        let mut a = app();
        let t = a.project.add_track(TrackKind::Midi);
        let id = midi_clip(&mut a.project, t, 0.0, 4.0, 60);
        a.arr_sel = vec![id];
        a.toggle_disable();
        assert!(a.project.tracks[t].arrangement[0].clip.muted);
        a.toggle_disable();
        assert!(!a.project.tracks[t].arrangement[0].clip.muted);
        // Notes when the piano roll has focus.
        a.selected_clip = Some(ClipRef::Arr { track: t, index: 0 });
        a.focus_detail = true;
        a.note_sel = vec![0];
        a.toggle_disable();
        let ClipContent::Midi { notes } = &a.project.tracks[t].arrangement[0].clip.content else { panic!() };
        assert!(notes[0].muted);
        assert!(!a.project.tracks[t].arrangement[0].clip.muted);
    }

    #[test]
    fn duplicate_notes_in_piano_roll() {
        let mut a = app();
        let t = a.project.add_track(TrackKind::Midi);
        midi_clip(&mut a.project, t, 0.0, 1.0, 60);
        a.selected_clip = Some(ClipRef::Arr { track: t, index: 0 });
        a.note_sel = vec![0];
        assert!(!a.duplicate_selected_notes(), "needs piano-roll focus");
        a.focus_detail = true;
        a.pr_grid_now = 0.25;
        assert!(a.duplicate_selected_notes());
        let c = &a.project.tracks[t].arrangement[0].clip;
        let ClipContent::Midi { notes } = &c.content else { panic!() };
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].start, 0.5);
        assert_eq!(a.note_sel, vec![1]);
    }

    #[test]
    fn consolidate_midi_clips() {
        let mut a = app();
        let t = a.project.add_track(TrackKind::Midi);
        let c1 = midi_clip(&mut a.project, t, 0.0, 2.0, 60);
        let c2 = midi_clip(&mut a.project, t, 4.0, 2.0, 64);
        a.arr_sel = vec![c1];
        a.consolidate_selection(); // a single MIDI clip: nothing to merge
        assert_eq!(a.project.tracks[t].arrangement.len(), 2);
        a.arr_sel = vec![c1, c2];
        a.consolidate_selection();
        let arr = &a.project.tracks[t].arrangement;
        assert_eq!(arr.len(), 1);
        assert_eq!((arr[0].start, arr[0].clip.length), (0.0, 6.0));
        let ClipContent::Midi { notes } = &arr[0].clip.content else { panic!() };
        assert_eq!(notes.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), vec![(0.0, 60), (4.0, 64)]);
    }

    #[test]
    fn consolidate_audio_renders_new_sample() {
        let mut a = app();
        let sr = a.audio.info.sample_rate;
        let t = a.project.add_track(TrackKind::Audio);
        // 1 beat at 120 BPM of DC per clip.
        let frames = sr as usize / 2;
        let key = a.store_audio("test", &vec![0.5; frames * 2]);
        let mut ids = Vec::new();
        for start in [0.0, 2.0] {
            let id = a.project.alloc_id();
            let content = ClipContent::Audio { sample: key.clone(), offset_frames: 0.0, warp_bpm: Some(120.0), gain_db: 0.0 };
            a.project.tracks[t].arrangement.push(ArrangementClip { start, clip: Clip { id, length: 1.0, content, ..Default::default() } });
            ids.push(id);
        }
        a.project.tempo_bpm = 120.0;
        a.arr_sel = ids;
        a.consolidate_selection();
        let arr = &a.project.tracks[t].arrangement;
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].clip.length, 3.0);
        let ClipContent::Audio { sample, .. } = &arr[0].clip.content else { panic!() };
        let buf = a.cache.peek(sample).expect("rendered sample cached");
        assert_eq!(buf.len(), frames * 3);
        // Signal in beat 0 and 2, silence in beat 1.
        let l = &buf.left;
        assert!(l[frames / 2].abs() > 0.1, "clip 1 audible");
        assert!(l[frames + frames / 2].abs() < 1e-4, "gap silent");
        assert!(l[2 * frames + frames / 2].abs() > 0.1, "clip 2 audible");
    }

    #[test]
    fn loop_toggle_with_ctrl_l() {
        let mut a = app();
        let t = a.project.add_track(TrackKind::Midi);
        let id = midi_clip(&mut a.project, t, 4.0, 4.0, 60);
        a.arr_sel = vec![id];
        a.loop_selection();
        assert!(a.project.loop_enabled);
        assert_eq!((a.project.loop_start, a.project.loop_end), (4.0, 8.0));
        a.loop_selection();
        assert!(!a.project.loop_enabled);
    }

    #[test]
    fn play_restarts_from_start_point() {
        let mut a = app();
        a.set_playhead(8.0);
        assert_eq!(a.play_start, 8.0);
    }
}
