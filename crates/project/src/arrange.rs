//! Arrangement editing on time ranges / clip sets (split, move with overwrite,
//! duplicate, insert). `fpb` gives an audio clip's source frames per beat so
//! offsets stay sample-accurate when clips are cut.

use crate::{ArrangementClip, Clip, ClipContent, Project, TrackKind};

const EPS: f64 = 1e-6;

impl Project {
    /// Split every clip on `tracks` that crosses `start` or `end`. Returns the ids of
    /// all clips that now lie fully inside `[start, end)`.
    pub fn split_range(&mut self, tracks: &[usize], start: f64, end: f64, fpb: &impl Fn(&Clip) -> f64) -> Vec<u64> {
        let mut inside = Vec::new();
        if end - start <= EPS {
            return inside;
        }
        for &t in tracks {
            let Some(tr) = self.tracks.get(t) else { continue };
            if tr.kind == TrackKind::Group {
                continue;
            }
            let ids: Vec<u64> = tr.arrangement.iter().map(|a| a.clip.id).collect();
            for id in ids {
                // Right part of a split at `start` is the candidate for the inside piece.
                let id = self.split_arrangement_clip(t, id, start, fpb).unwrap_or(id);
                self.split_arrangement_clip(t, id, end, fpb);
            }
            inside.extend(
                self.tracks[t]
                    .arrangement
                    .iter()
                    .filter(|a| a.start >= start - EPS && a.start + a.clip.length <= end + EPS)
                    .map(|a| a.clip.id),
            );
        }
        inside
    }

    /// Delete material in `[start, end)` on `tracks`.
    pub fn delete_range(&mut self, tracks: &[usize], start: f64, end: f64, fpb: &impl Fn(&Clip) -> f64) {
        for &t in tracks {
            self.clear_arrangement_range(t, start, end, fpb);
        }
    }

    /// Move clips: `(clip id, destination track, new start)`. Material under the
    /// destinations is overwritten (Live). Clip ids are preserved.
    pub fn move_clips(&mut self, moves: &[(u64, usize, f64)], fpb: &impl Fn(&Clip) -> f64) {
        let mut taken: Vec<(ArrangementClip, usize, f64)> = Vec::with_capacity(moves.len());
        for &(id, dest, start) in moves {
            if let Some((t, i)) = self.find_arrangement_clip(id)
                && dest < self.tracks.len()
            {
                taken.push((self.tracks[t].arrangement.remove(i), dest, start.max(0.0)));
            }
        }
        for (mut a, dest, start) in taken {
            a.start = start;
            self.clear_arrangement_range(dest, start, start + a.clip.length, fpb);
            self.tracks[dest].arrangement.push(a);
        }
        for t in &mut self.tracks {
            t.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
    }

    /// Copy the material in `[start, end)` on `tracks` to `[end, 2*end-start)`.
    /// Returns the ids of the copies.
    pub fn duplicate_range(&mut self, tracks: &[usize], start: f64, end: f64, fpb: &impl Fn(&Clip) -> f64) -> Vec<u64> {
        let len = end - start;
        let mut out = Vec::new();
        if len <= EPS {
            return out;
        }
        for &t in tracks {
            let Some(tr) = self.tracks.get(t) else { continue };
            let pieces: Vec<ArrangementClip> = tr.arrangement.iter().filter_map(|a| trimmed(a, start, end, fpb)).collect();
            if pieces.is_empty() {
                continue;
            }
            self.clear_arrangement_range(t, end, end + len, fpb);
            for mut p in pieces {
                p.clip.id = self.alloc_id();
                p.start += len;
                out.push(p.clip.id);
                self.tracks[t].arrangement.push(p);
            }
            self.tracks[t].arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        out
    }

    /// Does `[start, end)` on `track` overlap any clip?
    pub fn range_has_clips(&self, track: usize, start: f64, end: f64) -> bool {
        self.tracks.get(track).is_some_and(|tr| tr.arrangement.iter().any(|a| a.start < end - EPS && a.start + a.clip.length > start + EPS))
    }

    /// New empty MIDI clip spanning `[start, end)`; refuses to overwrite existing clips.
    pub fn insert_midi_clip(&mut self, track: usize, start: f64, end: f64) -> Result<u64, &'static str> {
        let tr = self.tracks.get(track).ok_or("no such track")?;
        if tr.kind != TrackKind::Midi {
            return Err("not a MIDI track");
        }
        if end - start < 1.0 / 64.0 {
            return Err("selection is empty");
        }
        if self.range_has_clips(track, start, end) {
            return Err("the selection already contains clips");
        }
        let id = self.alloc_id();
        let name = self.next_clip_name(track);
        let tr = &mut self.tracks[track];
        tr.arrangement.push(ArrangementClip {
            start,
            clip: Clip {
                id,
                name,
                color: tr.color,
                length: end - start,
                looping: false,
                content: ClipContent::Midi { notes: Vec::new() },
                ..Default::default()
            },
        });
        tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        Ok(id)
    }
}

impl Project {
    /// Material dragged over by a resize overwrites neighbours (Live): clear
    /// `[start, end)` on the clip's track around clip `id`, keeping the clip itself.
    pub fn overwrite_around(&mut self, id: u64, fpb: &impl Fn(&Clip) -> f64) {
        let Some((t, i)) = self.find_arrangement_clip(id) else { return };
        let a = self.tracks[t].arrangement.remove(i);
        self.clear_arrangement_range(t, a.start, a.start + a.clip.length, fpb);
        self.tracks[t].arrangement.push(a);
        self.tracks[t].arrangement.sort_by(|x, y| x.start.total_cmp(&y.start));
    }

    /// Ctrl+J: merge MIDI clips `ids` (all on `track`) into one clip spanning
    /// `[start, end)`. Notes keep their song position; gaps stay empty. Notes of
    /// deactivated clips come along deactivated. Returns the new clip's id.
    pub fn consolidate_midi(&mut self, track: usize, ids: &[u64], start: f64, end: f64) -> Option<u64> {
        let tr = self.tracks.get(track)?;
        if tr.kind != TrackKind::Midi || end - start < 1.0 / 64.0 {
            return None;
        }
        let mut notes = Vec::new();
        let mut first: Option<Clip> = None;
        for a in tr.arrangement.iter().filter(|a| ids.contains(&a.clip.id)) {
            let ClipContent::Midi { notes: n } = &a.clip.content else { continue };
            let len = a.clip.length;
            // Looping clips repeat their content over their length.
            let loop_len = if a.clip.looping { a.clip.content_full().max(1.0 / 64.0) } else { f64::INFINITY };
            let mut offset = 0.0;
            while offset < len - EPS {
                for x in n.iter().filter(|x| x.start >= 0.0 && x.start < loop_len.min(len - offset) - EPS) {
                    let mut y = *x;
                    y.start = a.start + offset + x.start - start;
                    y.length = y.length.min(len - offset - x.start);
                    y.muted |= a.clip.muted;
                    if y.start >= -EPS && y.start < end - start {
                        y.start = y.start.max(0.0);
                        notes.push(y);
                    }
                }
                offset += loop_len;
            }
            if first.is_none() {
                first = Some(a.clip.clone());
            }
        }
        let first = first?;
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        let id = self.alloc_id();
        let tr = &mut self.tracks[track];
        tr.arrangement.retain(|a| !ids.contains(&a.clip.id));
        let clip = Clip {
            id,
            length: end - start,
            looping: false,
            content: ClipContent::Midi { notes },
            muted: false,
            head: 0.0,
            full: 0.0,
            ..first
        };
        tr.arrangement.push(ArrangementClip { start, clip });
        tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        Some(id)
    }
}

/// Clip resize from the left edge to `new_start` (clamped to the material and
/// to keep at least `min_len`). Content stays at its song position.
pub fn trim_left(orig: &ArrangementClip, new_start: f64, min_len: f64, fpb: f64, total_frames: Option<f64>) -> ArrangementClip {
    let (max_ext, _) = orig.clip.extend_limits(fpb, total_frames);
    let end = orig.start + orig.clip.length;
    let s = new_start.max(orig.start - max_ext).max(0.0).min(end - min_len.min(orig.clip.length));
    let d = s - orig.start; // > 0: shorter
    let mut a = orig.clone();
    a.start = s;
    a.clip.length = end - s;
    a.clip.cut_front(d, fpb);
    if let (false, ClipContent::Midi { .. }) = (orig.clip.has_loop(), &a.clip.content) {
        a.clip.head = (orig.clip.head + d).max(0.0);
        a.clip.full = orig.clip.content_full();
    }
    a
}

/// Clip resize from the right edge to `len` beats (clamped like [`trim_left`]).
pub fn trim_right(orig: &ArrangementClip, len: f64, min_len: f64, fpb: f64, total_frames: Option<f64>) -> ArrangementClip {
    let (_, max_ext) = orig.clip.extend_limits(fpb, total_frames);
    let mut a = orig.clone();
    a.clip.length = len.min(orig.clip.length + max_ext).max(min_len.min(orig.clip.length));
    if let ClipContent::Midi { .. } = a.clip.content {
        a.clip.full = orig.clip.content_full();
    }
    a
}

/// The part of `a` inside `[start, end)` (same id; content shifted to stay in place).
fn trimmed(a: &ArrangementClip, start: f64, end: f64, fpb: &impl Fn(&Clip) -> f64) -> Option<ArrangementClip> {
    let (s, e) = (a.start.max(start), (a.start + a.clip.length).min(end));
    if e - s <= EPS {
        return None;
    }
    let cut = s - a.start;
    let mut c = a.clone();
    c.start = s;
    c.clip.length = e - s;
    c.clip.clear_extent();
    let s0 = c.clip.start;
    c.clip.cut_front(cut, fpb(&a.clip));
    if let (false, ClipContent::Midi { notes }) = (c.clip.has_loop(), &mut c.clip.content) {
        notes.retain(|n| n.start >= s0 - EPS && n.start < s0 + (e - s));
        notes.iter_mut().for_each(|n| n.start = n.start.max(s0));
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn proj() -> Project {
        let mut p = Project::new_default();
        p.tracks.clear();
        p.add_track(TrackKind::Midi);
        p.add_track(TrackKind::Midi);
        p
    }

    fn put(p: &mut Project, t: usize, start: f64, len: f64, notes: &[f64]) -> u64 {
        let id = p.alloc_id();
        let notes = notes.iter().map(|&s| Note { pitch: 60, start: s, length: 0.25, velocity: 100, muted: false, mpe: 0 }).collect();
        p.tracks[t].arrangement.push(ArrangementClip {
            start,
            clip: Clip { id, length: len, looping: false, content: ClipContent::Midi { notes }, ..Default::default() },
        });
        p.tracks[t].arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        id
    }

    fn spans(p: &Project, t: usize) -> Vec<(f64, f64)> {
        p.tracks[t].arrangement.iter().map(|a| (a.start, a.start + a.clip.length)).collect()
    }

    #[test]
    fn split_range_then_move_separates_the_middle() {
        let mut p = proj();
        put(&mut p, 0, 0.0, 8.0, &[1.0, 3.0, 5.0]);
        let ids = p.split_range(&[0, 1], 2.0, 4.0, &|_| 1.0);
        assert_eq!(ids.len(), 1);
        assert_eq!(spans(&p, 0), vec![(0.0, 2.0), (2.0, 4.0), (4.0, 8.0)]);
        // Drag the middle piece to track 1 at beat 10.
        p.move_clips(&[(ids[0], 1, 10.0)], &|_| 1.0);
        assert_eq!(spans(&p, 0), vec![(0.0, 2.0), (4.0, 8.0)]);
        assert_eq!(spans(&p, 1), vec![(10.0, 12.0)]);
        let ClipContent::Midi { notes } = &p.tracks[1].arrangement[0].clip.content else { panic!() };
        assert_eq!(notes.len(), 1);
        assert!((notes[0].start - 1.0).abs() < 1e-9, "note at beat 3 → 1 into the piece");
    }

    #[test]
    fn move_overwrites_destination() {
        let mut p = proj();
        let a = put(&mut p, 0, 0.0, 4.0, &[]);
        put(&mut p, 0, 6.0, 4.0, &[]);
        p.move_clips(&[(a, 0, 5.0)], &|_| 1.0);
        assert_eq!(spans(&p, 0), vec![(5.0, 9.0), (9.0, 10.0)]);
        assert_eq!(p.tracks[0].arrangement[0].clip.id, a);
    }

    #[test]
    fn duplicate_range_and_insert_midi_clip() {
        let mut p = proj();
        put(&mut p, 0, 1.0, 2.0, &[0.5]);
        let ids = p.duplicate_range(&[0, 1], 0.0, 4.0, &|_| 1.0);
        assert_eq!(ids.len(), 1);
        assert_eq!(spans(&p, 0), vec![(1.0, 3.0), (5.0, 7.0)]);
        assert!(p.insert_midi_clip(0, 2.0, 4.0).is_err(), "overlaps");
        let id = p.insert_midi_clip(1, 2.0, 6.0).unwrap();
        assert_eq!(p.find_arrangement_clip(id), Some((1, 0)));
        assert_eq!(spans(&p, 1), vec![(2.0, 6.0)]);
        let g = p.add_track(TrackKind::Group);
        assert!(p.insert_midi_clip(g, 0.0, 1.0).is_err());
        // Delete a range across a clip.
        p.delete_range(&[1], 3.0, 4.0, &|_| 1.0);
        assert_eq!(spans(&p, 1), vec![(2.0, 3.0), (4.0, 6.0)]);
    }

    #[test]
    fn notes_past_the_end_extend_the_right_limit() {
        // Shortening keeps notes beyond the end; dragging back out may reach them.
        let notes = vec![Note { start: 6.0, length: 1.0, pitch: 60, velocity: 100, muted: false, mpe: 0 }];
        let c = Clip { length: 4.0, looping: false, content: ClipContent::Midi { notes }, ..Default::default() };
        assert_eq!(c.extend_limits(0.0, None).1, 3.0);
        let a = ArrangementClip { start: 0.0, clip: c };
        let short = trim_right(&a, 2.0, 0.25, 0.0, None);
        assert!(matches!(&short.clip.content, ClipContent::Midi { notes } if notes.len() == 1));
        assert_eq!(trim_right(&short, 99.0, 0.25, 0.0, None).clip.length, 7.0);
    }

    #[test]
    fn resize_is_clamped_to_the_material() {
        // MIDI: shrink from both sides, then drag back out: stops at the original edges.
        let notes = vec![Note { start: 0.5, length: 0.5, pitch: 60, velocity: 100, muted: false, mpe: 0 }];
        let a = ArrangementClip {
            start: 4.0,
            clip: Clip { length: 4.0, looping: false, content: ClipContent::Midi { notes }, ..Default::default() },
        };
        assert_eq!(trim_right(&a, 9.0, 0.25, 0.0, None).clip.length, 4.0, "can't grow a fresh clip");
        let short = trim_right(&a, 2.0, 0.25, 0.0, None);
        assert_eq!(short.clip.length, 2.0);
        assert_eq!(trim_right(&short, 10.0, 0.25, 0.0, None).clip.length, 4.0);
        let l = trim_left(&short, 5.0, 0.25, 0.0, None);
        assert_eq!((l.start, l.clip.length, l.clip.head), (5.0, 1.0, 1.0));
        let ClipContent::Midi { notes } = &l.clip.content else { panic!() };
        assert_eq!(notes[0].start, -0.5, "note stays at its song position");
        let back = trim_left(&l, 0.0, 0.25, 0.0, None);
        assert_eq!((back.start, back.clip.length), (4.0, 2.0));
        assert_eq!(trim_right(&back, 99.0, 0.25, 0.0, None).clip.length, 4.0);
        // Audio: 4 s sample at 48 kHz, 120 BPM → 24000 frames/beat → 8 beats of material.
        let audio = |off: f64| ArrangementClip {
            start: 8.0,
            clip: Clip {
                length: 4.0,
                looping: false,
                content: ClipContent::Audio { sample: "x".into(), offset_frames: off, warp_bpm: None, gain_db: 0.0 },
                ..Default::default()
            },
        };
        let total = Some(192_000.0);
        assert_eq!(trim_right(&audio(0.0), 20.0, 0.25, 24_000.0, total).clip.length, 8.0);
        assert_eq!(trim_left(&audio(0.0), 2.0, 0.25, 24_000.0, total).start, 8.0, "no material before the sample start");
        let l = trim_left(&audio(48_000.0), 0.0, 0.25, 24_000.0, total);
        assert_eq!((l.start, l.clip.length), (6.0, 6.0));
        assert!(matches!(l.clip.content, ClipContent::Audio { offset_frames, .. } if offset_frames == 0.0));
    }

    #[test]
    fn consolidate_merges_midi_clips() {
        let mut p = Project::default();
        let t = p.add_track(TrackKind::Midi);
        let a = p.insert_midi_clip(t, 0.0, 2.0).unwrap();
        let b = p.insert_midi_clip(t, 4.0, 6.0).unwrap();
        for (id, pitch) in [(a, 60), (b, 62)] {
            let (_, i) = p.find_arrangement_clip(id).unwrap();
            let c = &mut p.tracks[t].arrangement[i].clip;
            if let ClipContent::Midi { notes } = &mut c.content {
                notes.push(Note { start: 1.0, length: 0.5, pitch, velocity: 90, muted: false, mpe: 0 });
            }
            c.muted = pitch == 62;
        }
        let id = p.consolidate_midi(t, &[a, b], 0.0, 6.0).unwrap();
        let arr = &p.tracks[t].arrangement;
        assert_eq!(arr.len(), 1);
        assert_eq!((arr[0].start, arr[0].clip.length, arr[0].clip.id), (0.0, 6.0, id));
        let ClipContent::Midi { notes } = &arr[0].clip.content else { panic!() };
        assert_eq!(notes.iter().map(|n| (n.start, n.pitch, n.muted)).collect::<Vec<_>>(), vec![(1.0, 60, false), (5.0, 62, true)]);
    }
}
