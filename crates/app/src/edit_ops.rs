//! Clipboard (clips / time ranges / tracks), arrangement zoom-to-selection and
//! the grid commands from Live's clip-area context menu (Ctrl+1..4).

use crate::app::DawApp;
use crate::arr_ops::{EPS, GridMode, TimeSel};
use daw_project::{ArrangementClip, Track};

/// Internal clipboard. The system clipboard only gets a marker text, so that
/// Ctrl+V reaches the app (egui only reports a paste when text is present).
#[derive(Clone, Debug, Default)]
pub enum Clipboard {
    #[default]
    Empty,
    /// Arrangement material: `(track offset from the first track, piece)`,
    /// piece starts relative to the copied range start; `len` = range length.
    Clips { pieces: Vec<(usize, ArrangementClip)>, len: f64 },
    /// Whole tracks (subtrees, roots detached).
    Tracks(Vec<Track>),
}

pub const CLIP_MARK: &str = "[daw clipboard]";

impl DawApp {
    // ----- clipboard ------------------------------------------------------------------

    /// Ctrl+C. Header focus with selected tracks copies tracks, else the time
    /// selection / selected clips. Returns a description for the system clipboard.
    pub fn copy_selection(&mut self) -> Option<String> {
        if self.head_focus && !self.focus_detail {
            let tracks = self.selected_tracks();
            if !tracks.is_empty() {
                let copy = self.project.copy_tracks(&tracks);
                let n = copy.iter().filter(|t| t.group.is_none()).count();
                self.clipboard = Clipboard::Tracks(copy);
                return Some(format!("{CLIP_MARK} {n} track(s)"));
            }
        }
        let fpb = self.fpb_fn();
        if let Some(ts) = self.time_sel.filter(|t| t.len() > EPS) {
            let tracks = self.time_sel_tracks(&ts);
            let first = *tracks.first()?;
            let pieces = self.project.copy_range(&tracks, ts.start, ts.end, &fpb);
            let pieces = pieces.into_iter().map(|(t, a)| (self.row_offset(first, t), a)).collect();
            self.clipboard = Clipboard::Clips { pieces, len: ts.len() };
            return Some(format!("{CLIP_MARK} time selection"));
        }
        let p = &self.project;
        let mut sel: Vec<(usize, ArrangementClip)> = self
            .arr_sel
            .iter()
            .filter_map(|&id| p.find_arrangement_clip(id))
            .map(|(t, i)| (t, p.tracks[t].arrangement[i].clone()))
            .collect();
        if sel.is_empty() {
            return None;
        }
        let start = sel.iter().map(|x| x.1.start).fold(f64::MAX, f64::min);
        let end = sel.iter().map(|x| x.1.start + x.1.clip.length).fold(f64::MIN, f64::max);
        let first = sel.iter().map(|x| x.0).min().unwrap_or(0);
        for (t, a) in &mut sel {
            a.start -= start;
            *t = self.row_offset(first, *t);
        }
        let n = sel.len();
        self.clipboard = Clipboard::Clips { pieces: sel, len: end - start };
        Some(format!("{CLIP_MARK} {n} clip(s)"))
    }

    /// Ctrl+X: copy, then delete what was copied.
    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.copy_selection()?;
        match self.clipboard {
            Clipboard::Tracks(_) => {
                self.delete_selected_tracks();
            }
            _ => {
                self.delete_arrangement_selection();
            }
        }
        Some(text)
    }

    /// Ctrl+V. Tracks go below the selected track; clips go to the start
    /// marker (or the time selection start) on the selected track(s).
    pub fn paste(&mut self) -> bool {
        match self.clipboard.clone() {
            Clipboard::Empty => false,
            Clipboard::Tracks(tracks) => {
                let after = (!self.master_selected).then(|| self.selected_tracks().into_iter().max()).flatten();
                let roots = self.edit(|p| p.paste_tracks(&tracks, after));
                self.clear_structure_selection();
                if let Some(&first) = roots.first() {
                    self.selected_track = first;
                    self.track_sel = roots;
                    self.head_focus = true;
                }
                true
            }
            Clipboard::Clips { pieces, len } => {
                let base = self.selected_tracks().into_iter().min().or(self.touched_track).unwrap_or(0);
                let at = self.time_sel.map_or(self.play_start, |ts| ts.start);
                let rows = self.project.visible_tracks();
                let Some(r0) = rows.iter().position(|&r| r == base) else { return false };
                let placed: Vec<(usize, ArrangementClip)> =
                    pieces.into_iter().filter_map(|(off, a)| rows.get(r0 + off).map(|&t| (t, a))).collect();
                if placed.is_empty() {
                    return false;
                }
                let fpb = self.fpb_fn();
                let ids = self.edit(|p| p.paste_clips(&placed, at, &fpb));
                if ids.is_empty() {
                    self.toast("Nothing to paste on these tracks (audio ↔ MIDI)");
                    return false;
                }
                self.arr_sel = ids;
                self.time_sel = None;
                self.resync_selected_clip();
                // Live: the insert marker moves to the end of the pasted material.
                self.set_start_marker(at + len);
                true
            }
        }
    }

    /// Position of `t` relative to `first` among the visible rows.
    fn row_offset(&self, first: usize, t: usize) -> usize {
        let rows = self.project.visible_tracks();
        match (rows.iter().position(|&r| r == first), rows.iter().position(|&r| r == t)) {
            (Some(a), Some(b)) if b >= a => b - a,
            _ => 0,
        }
    }

    // ----- zoom -------------------------------------------------------------------------

    /// Z: fit the time selection into the lanes (remembering the old view).
    pub fn zoom_to_selection(&mut self) {
        let Some(ts) = self.time_sel.filter(|t| t.len() > EPS) else { return };
        if self.zoom_back.is_none() {
            self.zoom_back = Some((self.arr_zoom, self.arr_scroll));
        }
        let w = self.arr_lanes_w.max(100.0);
        self.arr_zoom = (w / ts.len() as f32).clamp(1.5, 600.0);
        self.arr_scroll = ts.start as f32;
        self.follow_paused |= self.status.playing;
    }

    /// X: back to the view before [`Self::zoom_to_selection`].
    pub fn zoom_back_from_selection(&mut self) {
        if let Some((z, s)) = self.zoom_back.take() {
            self.arr_zoom = z;
            self.arr_scroll = s;
        }
    }

    // ----- grid -------------------------------------------------------------------------

    /// Ctrl+4: snapping on/off (the previous mode comes back).
    pub fn toggle_snap(&mut self) {
        if self.arr_grid == GridMode::Off {
            self.arr_grid = self.arr_grid_prev;
        } else {
            self.arr_grid_prev = self.arr_grid;
            self.arr_grid = GridMode::Off;
        }
    }

    /// Ctrl+1 (finer) / Ctrl+2 (coarser).
    pub fn narrow_grid(&mut self, finer: bool) {
        self.arr_grid = match self.arr_grid {
            GridMode::Adaptive(l) => GridMode::Adaptive(if finer { (l + 1).min(2) } else { (l - 1).max(-2) }),
            GridMode::Fixed(g) => {
                let all = GridMode::FIXED;
                let i = all.iter().position(|x| (x.0 - g).abs() < 1e-9).unwrap_or(4);
                let j = if finer { (i + 1).min(all.len() - 1) } else { i.saturating_sub(1) };
                GridMode::Fixed(all[j].0)
            }
            GridMode::Off => GridMode::Off,
        };
    }

    /// Shift+click in the lanes: time selection from the start marker (or the
    /// existing selection's far edge) to `beat`, across the tracks from the
    /// marker's tracks to `track`.
    pub fn extend_time_selection(&mut self, beat: f64, track: usize) {
        let rows = self.project.visible_tracks();
        let pos = |t: usize| rows.iter().position(|&r| r == t);
        let (anchor, mut tracks) = match self.time_sel {
            Some(ts) if ts.len() > EPS => {
                let a = if (beat - ts.start).abs() < (beat - ts.end).abs() { ts.end } else { ts.start };
                (a, vec![ts.t0, ts.t1])
            }
            _ => (self.play_start, self.selected_tracks()),
        };
        tracks.push(track);
        let idx: Vec<usize> = tracks.iter().filter_map(|&t| pos(t)).collect();
        let (Some(&lo), Some(&hi)) = (idx.iter().min(), idx.iter().max()) else { return };
        let ts = TimeSel::new(anchor, beat, rows[lo], rows[hi]);
        if ts.len() < EPS {
            return;
        }
        self.time_sel = Some(ts);
        self.set_start_marker(ts.start);
        self.arr_sel.clear();
        self.track_sel = rows[lo..=hi].to_vec();
        self.selected_track = track;
        self.hide_sel = None;
        self.touched_track = Some(track);
        self.resync_selected_clip();
    }

    // ----- track menu -------------------------------------------------------------------

    /// "Select Track Content": every clip of the track plus a time selection over them.
    pub fn select_track_content(&mut self, t: usize) {
        let Some(track) = self.project.tracks.get(t) else { return };
        let ids: Vec<u64> = track.arrangement.iter().map(|a| a.clip.id).collect();
        let start = track.arrangement.iter().map(|a| a.start).fold(f64::MAX, f64::min);
        let end = track.arrangement.iter().map(|a| a.start + a.clip.length).fold(f64::MIN, f64::max);
        if ids.is_empty() {
            return;
        }
        self.time_sel = Some(TimeSel::new(start, end, t, t));
        self.arr_sel = ids;
        self.track_sel = vec![t];
        self.selected_track = t;
        self.touched_track = Some(t);
        self.resync_selected_clip();
    }

    /// "Assign Track Color to Clips" (arrangement and Session clips).
    pub fn assign_track_color_to_clips(&mut self, tracks: &[usize]) {
        self.edit(|p| {
            for &t in tracks {
                let Some(tr) = p.tracks.get_mut(t) else { continue };
                let c = tr.color;
                tr.arrangement.iter_mut().for_each(|a| a.clip.color = c);
                tr.slots.iter_mut().flatten().for_each(|x| x.color = c);
            }
        });
    }

    /// Track colour from the context-menu grid, for every selected track.
    pub fn set_tracks_color(&mut self, tracks: &[usize], c: [u8; 3]) {
        self.edit(|p| {
            for &t in tracks {
                if let Some(tr) = p.tracks.get_mut(t) {
                    tr.color = c;
                }
            }
        });
    }
}
