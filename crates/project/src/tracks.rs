//! Track-level editing: display names, clip naming, reordering by drag.

use crate::{Project, TrackKind, display_name};

/// Where dragged tracks land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackDrop {
    /// Before the track with this id (joins that track's parent group); None = at the end, top level.
    Before(Option<u64>),
    /// Last child of this group.
    Into(u64),
}

/// Track name without the position number: `# MIDI` → `MIDI`, and a literal
/// leading "<position> " (typed when renaming) is dropped too.
pub fn base_name(name: &str, index: usize) -> &str {
    let n = name.strip_prefix('#').unwrap_or(name).trim_start();
    let num = (index + 1).to_string();
    match n.strip_prefix(num.as_str()) {
        Some(rest) if rest.starts_with(' ') && !rest.trim().is_empty() => rest.trim(),
        _ => n.trim(),
    }
}

impl Project {
    /// Name as shown in the UI (`#` → position).
    pub fn track_label(&self, i: usize) -> String {
        self.tracks.get(i).map_or_else(String::new, |t| display_name(&t.name, i))
    }

    /// "<track name> <n>" with n one past the highest number already used on that
    /// track. The position number shown in the header (`#`) is left out.
    pub fn next_clip_name(&self, track: usize) -> String {
        let label = self.tracks.get(track).map_or_else(String::new, |t| base_name(&t.name, track).to_string());
        let Some(t) = self.tracks.get(track) else { return label };
        let prefix = format!("{label} ");
        let n = t
            .slots
            .iter()
            .flatten()
            .chain(t.arrangement.iter().map(|a| &a.clip))
            .filter_map(|c| c.name.strip_prefix(&prefix)?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("{label} {}", n + 1)
    }

    /// Move tracks (groups with their children) to `to`. Returns the new indices
    /// of the moved top-level tracks, or None if the move is impossible (e.g. into itself).
    pub fn move_tracks(&mut self, ids: &[u64], to: TrackDrop) -> Option<Vec<usize>> {
        let mut roots: Vec<usize> = ids.iter().filter_map(|&id| self.track_index(id)).collect();
        roots.sort_unstable();
        roots.dedup();
        let all = roots.clone();
        roots.retain(|&i| !all.iter().any(|&a| a != i && self.is_descendant(i, a)));
        if roots.is_empty() {
            return None;
        }
        // Everything that moves (roots + descendants).
        let mut moving = vec![false; self.tracks.len()];
        for &r in &roots {
            for m in moving.iter_mut().take(self.subtree_end(r)).skip(r) {
                *m = true;
            }
        }
        let (anchor, parent) = match to {
            TrackDrop::Into(g) => {
                let gi = self.track_index(g)?;
                if self.tracks[gi].kind != TrackKind::Group || moving[gi] {
                    return None;
                }
                (None, Some(g))
            }
            TrackDrop::Before(Some(id)) => {
                let i = self.track_index(id)?;
                if moving[i] {
                    // Dropped onto the moved block itself: insert before the first track after it.
                    let after = (i..self.tracks.len()).find(|&j| !moving[j]);
                    let a = after.map(|j| self.tracks[j].id);
                    let parent = after.and_then(|j| self.tracks[j].group);
                    (a, parent)
                } else {
                    (Some(id), self.tracks[i].group)
                }
            }
            TrackDrop::Before(None) => (None, None),
        };
        let root_ids: Vec<u64> = roots.iter().map(|&r| self.tracks[r].id).collect();
        let mut block = Vec::new();
        let mut rest = Vec::new();
        for (t, m) in std::mem::take(&mut self.tracks).into_iter().zip(&moving) {
            if *m { block.push(t) } else { rest.push(t) }
        }
        for t in &mut block {
            if root_ids.contains(&t.id) {
                t.group = parent;
            }
        }
        let at = match (anchor, to) {
            (Some(a), _) => rest.iter().position(|t| t.id == a).unwrap_or(rest.len()),
            (None, TrackDrop::Into(g)) => {
                // After the group's last descendant.
                let gi = rest.iter().position(|t| t.id == g).unwrap_or(rest.len());
                self.tracks = rest;
                let end = self.subtree_end(gi);
                rest = std::mem::take(&mut self.tracks);
                end
            }
            (None, _) => rest.len(),
        };
        rest.splice(at..at, block);
        self.tracks = rest;
        self.normalize();
        Some(root_ids.iter().filter_map(|&id| self.track_index(id)).collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn blank(n: usize) -> Project {
        let mut p = Project::default();
        for _ in 0..n {
            p.add_track(TrackKind::Midi);
        }
        p
    }

    #[test]
    fn labels_follow_position_unless_renamed() {
        let mut p = blank(3);
        assert_eq!(p.track_label(2), "3 MIDI");
        p.tracks[0].name = "Bass".into();
        let ids: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        // Move the last track to the top: it becomes "1 MIDI", the renamed one keeps its name.
        p.move_tracks(&[ids[2]], TrackDrop::Before(Some(ids[0]))).unwrap();
        assert_eq!(p.tracks[0].id, ids[2]);
        assert_eq!(p.track_label(0), "1 MIDI");
        assert_eq!(p.track_label(1), "Bass");
        assert_eq!(p.track_label(2), "3 MIDI");
        p.tracks[1].name = "# Bass".into();
        assert_eq!(p.track_label(1), "2 Bass");
        assert!(p.tracks.iter().all(|t| t.instrument == InstrumentDef::None), "new MIDI tracks are empty");
    }

    #[test]
    fn clip_names_count_up_per_track() {
        let mut p = blank(2);
        p.add_scene();
        p.add_scene();
        assert_eq!(p.next_clip_name(1), "MIDI 1");
        p.new_midi_clip(1, 0, 4.0).unwrap();
        assert_eq!(p.tracks[1].slots[0].as_ref().unwrap().name, "MIDI 1");
        p.insert_midi_clip(1, 0.0, 4.0).unwrap();
        assert_eq!(p.tracks[1].arrangement[0].clip.name, "MIDI 2");
        assert_eq!(p.next_clip_name(0), "MIDI 1");
        p.tracks[0].name = "Bass".into();
        assert_eq!(p.next_clip_name(0), "Bass 1");
        p.tracks[0].name = "1 Drums".into();
        assert_eq!(p.next_clip_name(0), "Drums 1");
        assert_eq!(super::base_name("2 Drums", 0), "2 Drums");
        assert_eq!(super::base_name("1", 0), "1");
    }

    #[test]
    fn move_into_and_out_of_groups() {
        let mut p = blank(4); // a b c d
        let ids: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        let g = p.group_tracks(&[1, 2]).unwrap(); // a G(b c) d
        let gid = p.tracks[g].id;
        // d into the group → last child.
        p.move_tracks(&[ids[3]], TrackDrop::Into(gid)).unwrap();
        let order: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![ids[0], gid, ids[1], ids[2], ids[3]]);
        assert_eq!(p.tracks[4].group, Some(gid));
        // b out of the group, to the very end.
        p.move_tracks(&[ids[1]], TrackDrop::Before(None)).unwrap();
        assert_eq!(p.tracks.last().unwrap().id, ids[1]);
        assert_eq!(p.tracks.last().unwrap().group, None);
        // a before c → joins the group.
        p.move_tracks(&[ids[0]], TrackDrop::Before(Some(ids[2]))).unwrap();
        let ai = p.track_index(ids[0]).unwrap();
        assert_eq!(p.tracks[ai].group, Some(gid));
        // The group can't go into itself; moving it takes its children along.
        assert!(p.move_tracks(&[gid], TrackDrop::Into(gid)).is_none());
        p.move_tracks(&[gid], TrackDrop::Before(None)).unwrap();
        assert_eq!(p.tracks[0].id, ids[1]);
        assert_eq!(p.tracks[1].id, gid);
        assert_eq!(p.subtree_end(1), p.tracks.len());
    }

    #[test]
    fn old_auto_names_migrate() {
        let text = r#"(format_version: 2, tracks: [(id: 1, name: "1 MIDI"), (id: 2, name: "Lead"), (id: 3, name: "12 Audio")])"#;
        let p = Project::from_ron(text).unwrap();
        assert_eq!(p.tracks[0].name, "# MIDI");
        assert_eq!(p.tracks[1].name, "Lead");
        assert_eq!(p.track_label(2), "3 Audio");
    }
}
