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
        let tr = &mut self.tracks[track];
        tr.arrangement.push(ArrangementClip {
            start,
            clip: Clip {
                id,
                name: format!("Clip {id}"),
                color: tr.color,
                length: end - start,
                looping: false,
                content: ClipContent::Midi { notes: Vec::new() },
            },
        });
        tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        Ok(id)
    }
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
    match &mut c.clip.content {
        ClipContent::Midi { notes } => {
            notes.retain(|n| n.start >= cut - EPS && n.start < cut + (e - s));
            notes.iter_mut().for_each(|n| n.start = (n.start - cut).max(0.0));
        }
        ClipContent::Audio { offset_frames, .. } => *offset_frames += cut * fpb(&a.clip),
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
        let notes = notes.iter().map(|&s| Note { pitch: 60, start: s, length: 0.25, velocity: 100 }).collect();
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
}
