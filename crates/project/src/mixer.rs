//! v0.6.1 helpers: track insertion point, solo audibility, frame-rate
//! rescaling and warped-clip stretching.

use crate::{ArrangementClip, AudioOut, ClipContent, Project, SEND_OFF_DB, Track, TrackKind};

impl Project {
    /// Insert a new track below `after` (Live: below the selected track). A
    /// selected group gets the new track after its whole subtree, as a sibling.
    pub fn insert_track(&mut self, kind: TrackKind, after: Option<usize>) -> usize {
        let len = self.tracks.len();
        let (at, group) = match after.filter(|&i| i < len) {
            Some(i) if self.tracks[i].kind == TrackKind::Group => (self.subtree_end(i), self.tracks[i].group),
            Some(i) => (i + 1, self.tracks[i].group),
            None => (len, None),
        };
        let id = self.alloc_id();
        let name = match kind {
            TrackKind::Audio => "# Audio",
            TrackKind::Midi => "# MIDI",
            TrackKind::Group => "# Group",
        }
        .to_string();
        // New MIDI tracks start empty (drop an instrument from the browser).
        let t = Track { id, name, kind, group, color: self.auto_track_color(len), ..Default::default() };
        self.tracks.insert(at, t);
        self.normalize();
        self.track_index(id).unwrap_or(at)
    }

    /// Tracks silenced only because other tracks are soloed (engine rule: a
    /// soloed track keeps its destinations and sources audible).
    pub fn solo_silenced(&self) -> Vec<bool> {
        self.solo_silenced_all().0
    }

    /// Solo dimming for (tracks, returns), mirroring the engine: audible when
    /// soloed, feeding a soloed track (output), fed by a soloed track (output
    /// or send), or sending into a soloed return.
    pub fn solo_silenced_all(&self) -> (Vec<bool>, Vec<bool>) {
        let (n, nr) = (self.tracks.len(), self.returns.len());
        let all = n + nr;
        let solo: Vec<bool> = self.tracks.iter().chain(&self.returns).map(|t| t.solo).collect();
        if !solo.iter().any(|&s| s) {
            return (vec![false; n], vec![false; nr]);
        }
        let ret_index = |id: u64| self.returns.iter().position(|r| r.id == id).map(|k| n + k);
        let mut dest: Vec<Option<usize>> = vec![None; all];
        let mut sends: Vec<Vec<usize>> = vec![Vec::new(); all];
        for (i, t) in self.tracks.iter().enumerate() {
            if t.sends_midi() {
                continue;
            }
            dest[i] = match t.audio_out {
                AudioOut::Default => self.parent_index(i),
                AudioOut::Track(id) => self.track_index(id),
                _ => None,
            }
            .filter(|&d| d != i);
            sends[i] = t
                .sends
                .iter()
                .enumerate()
                .filter(|(_, db)| **db > SEND_OFF_DB)
                .filter_map(|(k, _)| self.returns.get(k))
                .filter_map(|r| ret_index(r.id))
                .collect();
        }
        let (mut up, mut down, mut feed) = (solo.clone(), solo.clone(), vec![false; all]);
        for _ in 0..all {
            let mut changed = false;
            for i in 0..all {
                let outs = dest[i].into_iter().chain(sends[i].iter().copied());
                for d in outs {
                    let via_out = dest[i] == Some(d);
                    if via_out && up[d] && !up[i] {
                        up[i] = true;
                        changed = true;
                    }
                    if down[i] && !down[d] {
                        down[d] = true;
                        changed = true;
                    }
                    if (up[d] || feed[d]) && !feed[i] && (!via_out || feed[d]) {
                        feed[i] = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let silenced: Vec<bool> = (0..all).map(|i| !(up[i] || down[i] || feed[i])).collect();
        (silenced[..n].to_vec(), silenced[n..].to_vec())
    }

    /// Sample positions were stored at `from` Hz; move them to `to` Hz.
    pub fn rescale_frames(&mut self, from: u32, to: u32) {
        if from == 0 || to == 0 || from == to {
            return;
        }
        let r = to as f64 / from as f64;
        for t in &mut self.tracks {
            for c in t.slots.iter_mut().flatten().chain(t.arrangement.iter_mut().map(|a| &mut a.clip)) {
                if let ClipContent::Audio { offset_frames, .. } = &mut c.content {
                    *offset_frames *= r;
                }
                c.audio.markers.iter_mut().for_each(|m| m.frame *= r);
                if let Some(tr) = &mut c.audio.transients {
                    tr.iter_mut().for_each(|f| *f *= r);
                }
            }
        }
    }
}

/// Shift+drag of a warped audio clip edge: stretch sample *and* clip to
/// `new_len` beats (left edge keeps the end in place). None = not warped audio.
pub fn stretch_clip(orig: &ArrangementClip, new_len: f64, left: bool) -> Option<ArrangementClip> {
    let ClipContent::Audio { warp_bpm: Some(bpm), .. } = orig.clip.content else { return None };
    let old = orig.clip.length;
    if old <= 1e-9 || bpm <= 0.0 {
        return None;
    }
    let new_len = new_len.max(1.0 / 64.0);
    let k = new_len / old;
    let mut a = orig.clone();
    let c = &mut a.clip;
    if let ClipContent::Audio { warp_bpm, .. } = &mut c.content {
        *warp_bpm = Some(bpm / k);
    }
    c.length = new_len;
    c.start *= k;
    if c.loop_end > c.loop_start + 1e-9 {
        c.loop_start *= k;
        c.loop_end *= k;
    }
    c.audio.markers.iter_mut().for_each(|m| m.beat *= k);
    for e in &mut c.envelopes {
        e.points.iter_mut().for_each(|p| p.beat *= k);
        e.loop_len *= k;
    }
    if left {
        a.start = (orig.start + old - new_len).max(0.0);
    }
    Some(a)
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn insert_below_selection_and_inside_groups() {
        let mut p = Project::new_default();
        p.tracks.clear();
        let a = p.insert_track(TrackKind::Audio, None);
        let b = p.insert_track(TrackKind::Audio, None);
        let c = p.insert_track(TrackKind::Midi, Some(a));
        assert_eq!((a, b, c), (0, 1, 1));
        assert_eq!(p.tracks[1].kind, TrackKind::Midi);
        // Inside a group: stays a child; on the group: after its subtree.
        let g = p.insert_track(TrackKind::Group, None);
        let gid = p.tracks[g].id;
        let ch = p.insert_track(TrackKind::Audio, Some(g));
        assert_eq!(p.tracks[ch].group, None);
        p.tracks[ch].group = Some(gid);
        let ch2 = p.insert_track(TrackKind::Midi, Some(ch));
        assert_eq!(p.tracks[ch2].group, Some(gid));
        let after = p.insert_track(TrackKind::Audio, Some(g));
        assert_eq!(after, ch2 + 1);
        assert_eq!(p.tracks[after].group, None);
    }

    #[test]
    fn solo_dims_only_unrelated_tracks() {
        let mut p = Project::new_default();
        p.tracks.clear();
        let g = p.add_track(TrackKind::Group);
        let gid = p.tracks[g].id;
        let c1 = p.add_track(TrackKind::Audio);
        let c2 = p.add_track(TrackKind::Audio);
        p.tracks[c1].group = Some(gid);
        p.tracks[c2].group = Some(gid);
        let other = p.add_track(TrackKind::Audio);
        assert!(p.solo_silenced().iter().all(|s| !s));
        p.tracks[c1].solo = true;
        let s = p.solo_silenced();
        assert!(!s[c1] && !s[g], "soloed track and its group bus stay lit");
        assert!(s[c2] && s[other]);
        p.tracks[c1].solo = false;
        p.tracks[g].solo = true;
        let s = p.solo_silenced();
        assert!(!s[c1] && !s[c2] && s[other], "a soloed group keeps its children");
    }

    #[test]
    fn rescale_and_stretch() {
        let mut p = Project::new_default();
        p.tracks.clear();
        let t = p.add_track(TrackKind::Audio);
        let mut clip = Clip {
            length: 4.0,
            content: ClipContent::Audio { sample: "x".into(), offset_frames: 1000.0, warp_bpm: Some(120.0), gain_db: 0.0 },
            ..Default::default()
        };
        clip.audio.markers = vec![clipx::WarpMarker { beat: 0.0, frame: 1000.0 }, clipx::WarpMarker { beat: 4.0, frame: 97000.0 }];
        p.tracks[t].arrangement.push(ArrangementClip { start: 8.0, clip });
        p.rescale_frames(48_000, 44_100);
        let c = &p.tracks[t].arrangement[0].clip;
        let ClipContent::Audio { offset_frames, .. } = c.content else { panic!() };
        assert!((offset_frames - 918.75).abs() < 1e-9 && (c.audio.markers[1].frame - 89118.75).abs() < 1e-6);
        let a = &p.tracks[t].arrangement[0];
        let r = stretch_clip(a, 8.0, false).unwrap();
        assert_eq!((r.start, r.clip.length, r.clip.audio.markers[1].beat), (8.0, 8.0, 8.0));
        assert!(matches!(r.clip.content, ClipContent::Audio { warp_bpm: Some(b), .. } if (b - 60.0).abs() < 1e-9));
        let l = stretch_clip(a, 2.0, true).unwrap();
        assert_eq!((l.start, l.clip.length), (10.0, 2.0));
        let mut un = a.clone();
        if let ClipContent::Audio { warp_bpm, .. } = &mut un.clip.content {
            *warp_bpm = None;
        }
        assert!(stretch_clip(&un, 8.0, false).is_none());
    }
}

#[cfg(test)]
mod color_policy_tests {
    use crate::{ColorPolicy, PALETTE, Project, TrackKind};

    #[test]
    fn track_and_clip_colors_follow_policy() {
        let mut p = Project::default();
        p.tracks.clear();
        let a = p.add_track(TrackKind::Midi);
        let b = p.add_track(TrackKind::Midi);
        assert_ne!(p.tracks[a].color, p.tracks[b].color, "auto-assign cycles the palette");
        p.colors = ColorPolicy { manual: Some([1, 2, 3]), ..Default::default() };
        let c = p.add_track(TrackKind::Audio);
        assert_eq!(p.tracks[c].color, [1, 2, 3], "auto-assign off uses the default colour");
        p.colors = ColorPolicy { reduced: true, ..Default::default() };
        assert!((0..20).all(|n| PALETTE.iter().step_by(2).any(|x| *x == p.auto_track_color(n))));
        // Clip colour: track colour by default, palette colours when random.
        p.colors = ColorPolicy::default();
        assert_eq!(p.new_clip_color(c), [1, 2, 3]);
        p.colors.random_clips = true;
        let cols: std::collections::HashSet<_> = (0..40).map(|_| p.new_clip_color(c)).collect();
        assert!(cols.len() > 3 && cols.iter().all(|x| PALETTE.contains(x)));
    }
}

#[cfg(test)]
mod return_solo_tests {
    use crate::*;

    #[test]
    fn soloed_return_keeps_senders_audible() {
        let mut p = Project::new_default();
        let a = p.add_track(TrackKind::Audio);
        let b = p.add_track(TrackKind::Audio);
        p.normalize();
        p.tracks[a].sends[0] = -6.0;
        p.returns[0].solo = true;
        let (t, r) = p.solo_silenced_all();
        assert!(!t[a], "sender stays");
        assert!(t[b], "non-sender dimmed");
        assert!(!r[0] && r[1]);
        p.returns[0].solo = false;
        p.tracks[a].solo = true;
        let (t, r) = p.solo_silenced_all();
        assert!(!t[a] && t[b] && !r[0] && r[1], "solo in place keeps the fed return");
    }
}
