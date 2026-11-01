//! Device chains: insert / delete / duplicate / move audio effects and live
//! parameter changes (sent straight to the engine, no recompile).

use crate::app::DawApp;
use daw_engine::{Command, MASTER_FX};
use daw_project::devices::FxParams;
use daw_project::{Device, Project};
use eframe::egui;

/// A device chain: a track or return (by id) or the Main track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChainRef {
    Track(u64),
    Main,
}

pub fn chain(p: &Project, c: ChainRef) -> &[Device] {
    match c {
        ChainRef::Main => &p.master_devices,
        ChainRef::Track(id) => p.tracks.iter().chain(&p.returns).find(|t| t.id == id).map(|t| t.devices.as_slice()).unwrap_or(&[]),
    }
}

pub fn chain_mut(p: &mut Project, c: ChainRef) -> Option<&mut Vec<Device>> {
    match c {
        ChainRef::Main => Some(&mut p.master_devices),
        ChainRef::Track(id) => p.tracks.iter_mut().chain(&mut p.returns).find(|t| t.id == id).map(|t| &mut t.devices),
    }
}

impl DawApp {
    /// The chain shown in the device view.
    pub fn current_chain(&self) -> ChainRef {
        if self.master_selected {
            ChainRef::Main
        } else if let Some(r) = self.selected_return.and_then(|k| self.project.returns.get(k)) {
            ChainRef::Track(r.id)
        } else {
            self.project.tracks.get(self.selected_track).map(|t| ChainRef::Track(t.id)).unwrap_or(ChainRef::Main)
        }
    }

    fn chain_rt(&self, c: ChainRef) -> Option<u16> {
        match c {
            ChainRef::Main => Some(MASTER_FX),
            ChainRef::Track(id) => self.slots.get(id),
        }
    }

    /// Set a device's activator / parameters (no undo step: call
    /// `begin_gesture` first). Reaches the engine immediately.
    pub fn set_device(&mut self, c: ChainRef, id: u64, on: bool, params: FxParams) {
        let Some(v) = chain_mut(&mut self.project, c) else { return };
        let Some(i) = v.iter().position(|d| d.id == id) else { return };
        if v[i].on == on && v[i].params == params {
            return;
        }
        v[i].on = on;
        v[i].params = params;
        self.dirty = true;
        if let Some(track) = self.chain_rt(c) {
            self.send_now(Command::SetFx { track, slot: i.min(255) as u8, on, params });
        }
    }

    /// Insert a new device at `at` (None: after the selected device, else at
    /// the end) and select it.
    pub fn insert_device(&mut self, c: ChainRef, at: Option<usize>, params: FxParams) {
        let sel = self.dev_sel;
        let id = self.edit(|p| {
            let id = p.alloc_id();
            let Some(v) = chain_mut(p, c) else { return None };
            let at = at.or_else(|| sel.and_then(|s| v.iter().position(|d| d.id == s)).map(|i| i + 1)).unwrap_or(v.len()).min(v.len());
            v.insert(at, Device::new(id, params));
            Some(id)
        });
        if id.is_some() {
            self.dev_sel = id;
            self.dev_focus = None;
            self.detail_mode = crate::app::DetailMode::Device;
        }
    }

    pub fn delete_device(&mut self, c: ChainRef, id: u64) {
        if !chain(&self.project, c).iter().any(|d| d.id == id) {
            return;
        }
        self.edit(|p| {
            if let Some(v) = chain_mut(p, c) {
                v.retain(|d| d.id != id);
            }
        });
        if self.dev_sel == Some(id) {
            self.dev_sel = None;
        }
        if self.dev_focus.is_some_and(|f| f.0 == id) {
            self.dev_focus = None;
        }
    }

    /// The detail panel shows the device view.
    pub fn device_view_shown(&self) -> bool {
        self.detail_mode == crate::app::DetailMode::Device || self.master_selected || self.selected_return.is_some()
    }

    /// Device-view keys: arrows step / Delete resets the focused parameter,
    /// Delete removes the selected device. Returns true when handled.
    pub fn device_key(&mut self, key: egui::Key, repeat: bool) -> bool {
        use egui::Key;
        let c = self.current_chain();
        if let Some((id, k)) = self.dev_focus
            && let Some(d) = chain(&self.project, c).iter().find(|d| d.id == id).copied()
        {
            let mut p = d.params;
            match key {
                Key::ArrowUp | Key::ArrowRight => crate::ui::devices_step(&mut p, k, 1.0),
                Key::ArrowDown | Key::ArrowLeft => crate::ui::devices_step(&mut p, k, -1.0),
                Key::Delete | Key::Backspace if !repeat => crate::ui::devices_reset(&mut p, k),
                _ => return false,
            }
            if p != d.params {
                if !repeat {
                    self.begin_gesture();
                }
                self.set_device(c, id, d.on, p);
            }
            return true;
        }
        if matches!(key, Key::Delete | Key::Backspace) && !repeat
            && let Some(id) = self.dev_sel.filter(|&id| chain(&self.project, c).iter().any(|d| d.id == id))
        {
            self.delete_device(c, id);
            return true;
        }
        false
    }

    pub fn duplicate_selected_device(&mut self) -> bool {
        let c = self.current_chain();
        let Some(id) = self.dev_sel.filter(|&id| chain(&self.project, c).iter().any(|d| d.id == id)) else { return false };
        self.duplicate_device(c, id);
        true
    }

    /// Ctrl+D: a copy right after the device, selected.
    pub fn duplicate_device(&mut self, c: ChainRef, id: u64) {
        let Some(i) = chain(&self.project, c).iter().position(|d| d.id == id) else { return };
        let new = self.edit(|p| {
            let nid = p.alloc_id();
            let v = chain_mut(p, c)?;
            let mut d = v[i];
            d.id = nid;
            v.insert(i + 1, d);
            Some(nid)
        });
        if new.is_some() {
            self.dev_sel = new;
        }
    }

    /// Move (or copy) a device to position `at` of chain `to`.
    pub fn move_device(&mut self, from: ChainRef, id: u64, to: ChainRef, at: usize, copy: bool) {
        let Some(i) = chain(&self.project, from).iter().position(|d| d.id == id) else { return };
        if !copy && from == to && (at == i || at == i + 1) {
            return;
        }
        let new = self.edit(|p| {
            let nid = p.alloc_id();
            let mut d = chain(p, from)[i];
            let mut at = at;
            if copy {
                d.id = nid;
            } else {
                chain_mut(p, from)?.remove(i);
                if from == to && at > i {
                    at -= 1;
                }
            }
            let v = chain_mut(p, to)?;
            v.insert(at.min(v.len()), d);
            Some(d.id)
        });
        if new.is_some() {
            self.dev_sel = new;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_lookup_covers_tracks_returns_main() {
        let mut p = Project::default();
        let t = p.add_track(daw_project::TrackKind::Audio);
        let id = p.tracks[t].id;
        chain_mut(&mut p, ChainRef::Track(id)).unwrap().push(Device::new(7, FxParams::Utility(Default::default())));
        chain_mut(&mut p, ChainRef::Main).unwrap().push(Device::new(8, FxParams::Utility(Default::default())));
        assert_eq!(chain(&p, ChainRef::Track(id))[0].id, 7);
        assert_eq!(chain(&p, ChainRef::Main)[0].id, 8);
        assert!(chain(&p, ChainRef::Track(999)).is_empty());
    }
}
