//! Snapshot-based undo/redo. A project is small (samples are referenced by
//! path), so cloning it per edit is cheap and makes undo always correct.

use crate::Project;

pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(200)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit: limit.max(1) }
    }

    /// Call *before* changing `current`.
    pub fn checkpoint(&mut self, current: &Project) {
        if self.undo.last() == Some(current) {
            return; // nothing changed since the last checkpoint
        }
        self.undo.push(current.clone());
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &mut Project) -> bool {
        // Skip snapshots identical to the current state.
        while let Some(prev) = self.undo.pop() {
            if prev != *current {
                self.redo.push(std::mem::replace(current, prev));
                return true;
            }
        }
        false
    }

    pub fn redo(&mut self, current: &mut Project) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(current, next));
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply `f` to every stored snapshot (e.g. a sample-rate change).
    pub fn for_each_snapshot(&mut self, mut f: impl FnMut(&mut Project)) {
        self.undo.iter_mut().chain(self.redo.iter_mut()).for_each(&mut f);
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_cycle() {
        let mut h = History::default();
        let mut p = Project::default();
        h.checkpoint(&p);
        p.tempo_bpm = 100.0;
        h.checkpoint(&p);
        p.tempo_bpm = 80.0;
        assert!(h.undo(&mut p));
        assert_eq!(p.tempo_bpm, 100.0);
        assert!(h.undo(&mut p));
        assert_eq!(p.tempo_bpm, 120.0);
        assert!(!h.undo(&mut p));
        assert!(h.redo(&mut p));
        assert!(h.redo(&mut p));
        assert_eq!(p.tempo_bpm, 80.0);
        assert!(!h.redo(&mut p));
        h.checkpoint(&p);
        p.tempo_bpm = 90.0;
        assert!(h.undo(&mut p));
        h.checkpoint(&p);
        assert!(!h.can_redo(), "new edit clears redo");
    }

    #[test]
    fn duplicate_checkpoints_are_skipped_and_limit_holds() {
        let mut h = History::new(3);
        let mut p = Project::default();
        for i in 0..10 {
            h.checkpoint(&p);
            h.checkpoint(&p);
            p.tempo_bpm = 100.0 + i as f64;
        }
        let mut undos = 0;
        while h.undo(&mut p) {
            undos += 1;
        }
        assert_eq!(undos, 3);
    }
}
