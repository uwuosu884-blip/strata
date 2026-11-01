//! Device chains: insert / delete / duplicate / move audio effects and live
//! parameter changes (sent straight to the engine, no recompile).

use crate::app::DawApp;
use daw_engine::{Command, MASTER_FX};
use daw_project::devices::{ChainMix, FxParams, MacroMap, RackChain, chain_owner, device_parent, find_chain, find_chain_mut, find_device};
use daw_project::{Device, Project};
use eframe::egui;

/// A device chain: a track or return (by id), the Main track, or a rack
/// chain (by chain id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChainRef {
    Track(u64),
    Main,
    Sub(u64),
}

fn tops(p: &Project) -> impl Iterator<Item = (ChainRef, &[Device])> {
    std::iter::once((ChainRef::Main, p.master_devices.as_slice()))
        .chain(p.tracks.iter().chain(&p.returns).map(|t| (ChainRef::Track(t.id), t.devices.as_slice())))
}

pub fn chain(p: &Project, c: ChainRef) -> &[Device] {
    match c {
        ChainRef::Main => &p.master_devices,
        ChainRef::Track(id) => p.tracks.iter().chain(&p.returns).find(|t| t.id == id).map(|t| t.devices.as_slice()).unwrap_or(&[]),
        ChainRef::Sub(id) => tops(p).find_map(|(_, v)| find_chain(v, id)).map(|c| c.devices.as_slice()).unwrap_or(&[]),
    }
}

pub fn chain_mut(p: &mut Project, c: ChainRef) -> Option<&mut Vec<Device>> {
    match c {
        ChainRef::Main => Some(&mut p.master_devices),
        ChainRef::Track(id) => p.tracks.iter_mut().chain(&mut p.returns).find(|t| t.id == id).map(|t| &mut t.devices),
        ChainRef::Sub(id) => {
            let top = top_of(p, c);
            let v = chain_mut(p, top)?;
            find_chain_mut(v, id).map(|c| &mut c.devices)
        }
    }
}

/// The track-level chain containing `c`.
pub fn top_of(p: &Project, c: ChainRef) -> ChainRef {
    match c {
        ChainRef::Sub(id) => tops(p).find(|(_, v)| find_chain(v, id).is_some()).map(|t| t.0).unwrap_or(ChainRef::Main),
        _ => c,
    }
}

/// The chain directly containing device `id`, searched below `top`.
pub fn chain_of(p: &Project, top: ChainRef, id: u64) -> Option<ChainRef> {
    let v = chain(p, top);
    if v.iter().any(|d| d.id == id) {
        return Some(top);
    }
    device_parent(v, id).map(|(_, c)| ChainRef::Sub(c.id))
}

/// Device `id` below `top`.
pub fn find_dev(p: &Project, top: ChainRef, id: u64) -> Option<&Device> {
    find_device(chain(p, top), id)
}

/// Rack devices enclosing chain `c` (innermost first).
pub fn racks_above(p: &Project, c: ChainRef) -> Vec<u64> {
    let mut out = Vec::new();
    let top = top_of(p, c);
    let v = chain(p, top);
    let mut cur = c;
    while let ChainRef::Sub(id) = cur {
        let Some(r) = chain_owner(v, id) else { break };
        out.push(r.id);
        cur = device_parent(v, r.id).map(|(_, c)| ChainRef::Sub(c.id)).unwrap_or(top);
    }
    out
}

/// Normalised value a mapping writes for macro value `v` (0..=127).
pub fn map_value(m: &MacroMap, v: f32) -> f32 {
    (m.min + (m.max - m.min) * (v / 127.0)).clamp(0.0, 1.0)
}

/// What a new rack chain starts with: a moved / copied device (chain, id,
/// copy) or a new device with these parameters.
pub type ChainSource = (Option<(ChainRef, u64, bool)>, FxParams);

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
            ChainRef::Sub(_) => self.chain_rt(top_of(&self.project, c)),
        }
    }

    /// Set a device's activator / parameters (no undo step: call
    /// `begin_gesture` first). Reaches the engine immediately. A rack's
    /// changed macros drive their mapped parameters.
    pub fn set_device(&mut self, c: ChainRef, id: u64, on: bool, params: FxParams) {
        let Some(v) = chain_mut(&mut self.project, c) else { return };
        let Some(i) = v.iter().position(|d| d.id == id) else { return };
        if v[i].on == on && v[i].params == params {
            return;
        }
        let old = std::mem::replace(&mut v[i].params, params);
        v[i].on = on;
        self.dirty = true;
        if let Some(track) = self.chain_rt(c) {
            let cmd = match c {
                ChainRef::Sub(_) => Command::SetFxById { track, id, on, params },
                _ => Command::SetFx { track, slot: i.min(255) as u8, on, params },
            };
            self.send_now(cmd);
        }
        if let (FxParams::Rack(a), FxParams::Rack(b)) = (old, params) {
            let changed: Vec<usize> = (0..a.macros.len()).filter(|&k| a.macros[k] != b.macros[k]).collect();
            if !changed.is_empty() {
                self.apply_macros(c, id, &changed);
            }
        }
    }

    /// Write macros `which` of rack `rack` (in chain `c`) to their targets.
    pub fn apply_macros(&mut self, c: ChainRef, rack: u64, which: &[usize]) {
        let Some(d) = chain(&self.project, c).iter().find(|d| d.id == rack) else { return };
        let (FxParams::Rack(rp), Some(r)) = (d.params, d.rack.as_deref()) else { return };
        let maps: Vec<MacroMap> = r.mappings.iter().filter(|m| which.contains(&(m.macro_idx as usize))).copied().collect();
        let top = top_of(&self.project, c);
        for m in maps {
            let Some(tc) = chain_of(&self.project, top, m.device) else { continue };
            let Some(t) = find_dev(&self.project, top, m.device) else { continue };
            let (on, mut p) = (t.on, t.params);
            crate::ui::devices_set_norm(&mut p, m.param, map_value(&m, rp.macros[m.macro_idx as usize]));
            self.set_device(tc, m.device, on, p);
        }
    }

    /// The macro mapped to parameter `k` of device `id` (rack id, macro).
    pub fn mapped(&self, c: ChainRef, id: u64, k: u8) -> Option<(u64, u8)> {
        let top = top_of(&self.project, c);
        for r in racks_above(&self.project, c) {
            let d = find_dev(&self.project, top, r)?;
            if let Some(m) = d.rack.as_ref()?.mappings.iter().find(|m| m.device == id && m.param == k) {
                return Some((r, m.macro_idx));
            }
        }
        None
    }

    /// Chain mixer / zone (no undo step). Reaches the engine immediately.
    pub fn set_chain_mix(&mut self, chain_id: u64, mix: ChainMix) {
        let c = ChainRef::Sub(chain_id);
        let top = top_of(&self.project, c);
        let Some(v) = chain_mut(&mut self.project, top) else { return };
        let Some(ch) = find_chain_mut(v, chain_id) else { return };
        if ch.mix == mix {
            return;
        }
        ch.mix = mix;
        self.dirty = true;
        if let Some(track) = self.chain_rt(top) {
            self.send_now(Command::SetRackChain { track, id: chain_id, mix });
        }
    }

    /// Structural rack edit (undoable, recompiles) on rack `rack` in `c`.
    pub fn edit_rack<R>(&mut self, c: ChainRef, rack: u64, f: impl FnOnce(&mut Project, &mut dyn FnMut() -> u64) -> R) -> Option<R> {
        chain(&self.project, c).iter().find(|d| d.id == rack && d.rack.is_some())?;
        Some(self.edit(|p| {
            let mut ids = Vec::new();
            for _ in 0..64 {
                ids.push(p.alloc_id());
            }
            let mut it = ids.into_iter();
            let mut alloc = move || it.next().unwrap_or(0);
            f(p, &mut alloc)
        }))
    }

    /// Ctrl+G: wrap device `id` of chain `c` in an Audio Effect Rack.
    pub fn group_device(&mut self, c: ChainRef, id: u64) {
        let Some(i) = chain(&self.project, c).iter().position(|d| d.id == id) else { return };
        let rid = self.edit(|p| {
            let (rid, cid) = (p.alloc_id(), p.alloc_id());
            let v = chain_mut(p, c)?;
            let d = v.remove(i);
            let mut rack = Device::new(rid, FxParams::Rack(Default::default()));
            if let Some(r) = rack.rack.as_deref_mut() {
                r.chains.push(RackChain { id: cid, devices: vec![d], ..Default::default() });
            }
            v.insert(i, rack);
            Some(rid)
        });
        if rid.is_some() {
            self.dev_sel = rid;
        }
    }

    /// Ctrl+Shift+G: replace rack `id` by its chains' devices in series.
    pub fn ungroup_device(&mut self, c: ChainRef, id: u64) {
        let Some(i) = chain(&self.project, c).iter().position(|d| d.id == id && d.rack.is_some()) else { return };
        self.edit(|p| {
            let Some(v) = chain_mut(p, c) else { return };
            let d = v.remove(i);
            let devs: Vec<Device> = d.rack.map(|r| r.chains.into_iter().flat_map(|ch| ch.devices).collect()).unwrap_or_default();
            for (k, x) in devs.into_iter().enumerate() {
                v.insert(i + k, x);
            }
        });
        self.dev_sel = None;
    }

    /// Insert a new device at `at` (None: after the selected device, else at
    /// the end) and select it.
    pub fn insert_device(&mut self, c: ChainRef, at: Option<usize>, params: FxParams) {
        let sel = self.dev_sel;
        let id = self.edit(|p| {
            let id = p.alloc_id();
            let v = chain_mut(p, c)?;
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
        let top = self.current_chain();
        if let Some((id, k)) = self.dev_focus
            && let Some(c) = chain_of(&self.project, top, id)
            && self.mapped(c, id, k).is_none()
            && let Some(d) = chain(&self.project, c).iter().find(|d| d.id == id).cloned()
        {
            // Digits / sign / point on a value box start typing a value.
            let typed = match key {
                Key::Num0 | Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 | Key::Num5 | Key::Num6 | Key::Num7 | Key::Num8 | Key::Num9 => {
                    Some(key.symbol_or_name().to_string())
                }
                Key::Minus => Some("-".to_string()),
                Key::Period => Some(".".to_string()),
                _ => None,
            };
            if let Some(t) = typed {
                if crate::ui::devices_typable(&d.params, k) {
                    self.dev_typing = Some(t);
                    return true;
                }
                return false;
            }
            let mut p = d.params;
            if let FxParams::EqEight(e) = &mut p
                && k == crate::ui::EQ_DOTS
                && matches!(key, Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight)
            {
                crate::ui::eq_arrow(e, key);
                if !repeat {
                    self.begin_gesture();
                }
                self.set_device(c, id, d.on, p);
                return true;
            }
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
        let Some((id, c)) = self.dev_sel.and_then(|id| Some((id, chain_of(&self.project, top, id)?))) else { return false };
        if matches!(key, Key::Delete | Key::Backspace) && !repeat {
            self.delete_device(c, id);
            return true;
        }
        // Rack selected: Up / Down walk its chain list.
        if let Some(d) = chain(&self.project, c).iter().find(|d| d.id == id)
            && let (FxParams::Rack(mut rp), Some(r)) = (d.params, d.rack.as_deref())
            && matches!(key, Key::ArrowUp | Key::ArrowDown)
            && !r.chains.is_empty()
        {
            let n = r.chains.len() as i32;
            let cur = (rp.sel_chain as i32).min(n - 1);
            rp.sel_chain = (cur + if key == Key::ArrowUp { -1 } else { 1 }).clamp(0, n - 1) as u16;
            let on = d.on;
            self.set_device(c, id, on, FxParams::Rack(rp));
            return true;
        }
        false
    }

    /// The selected device and its chain (any rack depth) in the device view.
    pub fn selected_device(&self) -> Option<(ChainRef, u64)> {
        let id = self.dev_sel?;
        Some((chain_of(&self.project, self.current_chain(), id)?, id))
    }

    pub fn duplicate_selected_device(&mut self) -> bool {
        let Some((c, id)) = self.selected_device() else { return false };
        self.duplicate_device(c, id);
        true
    }

    /// Ctrl+G / Ctrl+Shift+G on the selected device.
    pub fn group_selected_device(&mut self, ungroup: bool) -> bool {
        let Some((c, id)) = self.selected_device() else { return false };
        if ungroup {
            self.ungroup_device(c, id);
        } else {
            self.group_device(c, id);
        }
        true
    }

    /// Ctrl+D: a copy right after the device, selected.
    pub fn duplicate_device(&mut self, c: ChainRef, id: u64) {
        let Some(i) = chain(&self.project, c).iter().position(|d| d.id == id) else { return };
        let new = self.edit(|p| {
            let mut d = chain(p, c).get(i)?.clone();
            d.reid(&mut || p.alloc_id());
            let nid = d.id;
            chain_mut(p, c)?.insert(i + 1, d);
            Some(nid)
        });
        if new.is_some() {
            self.dev_sel = new;
        }
    }

    /// Move (or copy) a device to position `at` of chain `to`. A rack can't
    /// go into its own chains.
    pub fn move_device(&mut self, from: ChainRef, id: u64, to: ChainRef, at: usize, copy: bool) {
        let Some(i) = chain(&self.project, from).iter().position(|d| d.id == id) else { return };
        if !copy && from == to && (at == i || at == i + 1) {
            return;
        }
        if let ChainRef::Sub(cid) = to
            && chain(&self.project, from).get(i).is_some_and(|d| find_chain(std::slice::from_ref(d), cid).is_some())
        {
            return;
        }
        let new = self.edit(|p| {
            let mut d = chain(p, from).get(i)?.clone();
            let mut at = at;
            if copy {
                d.reid(&mut || p.alloc_id());
            } else {
                chain_mut(p, from)?.remove(i);
                if from == to && at > i {
                    at -= 1;
                }
            }
            let nid = d.id;
            let v = chain_mut(p, to)?;
            v.insert(at.min(v.len()), d);
            Some(nid)
        });
        if new.is_some() {
            self.dev_sel = new;
        }
    }

    /// New chain at the end of rack `rack` (in `c`) holding `dev` (moved
    /// from `from` or a new device); selects the chain.
    pub fn add_chain(&mut self, c: ChainRef, rack: u64, dev: Option<ChainSource>) {
        // A rack can't receive itself.
        if let Some((Some((_, id, _)), _)) = dev
            && (id == rack || racks_above(&self.project, c).contains(&id))
        {
            return;
        }
        let moved = dev.as_ref().and_then(|(m, _)| *m).and_then(|(fc, id, copy)| {
            let d = chain(&self.project, fc).iter().find(|d| d.id == id)?.clone();
            Some((fc, d, copy))
        });
        let nsel = self.edit(|p| {
            let cid = p.alloc_id();
            let mut devs = Vec::new();
            if let Some((fc, mut d, copy)) = moved {
                if copy {
                    d.reid(&mut || p.alloc_id());
                } else if let Some(v) = chain_mut(p, fc) {
                    v.retain(|x| x.id != d.id);
                }
                devs.push(d);
            } else if let Some((None, params)) = dev {
                devs.push(Device::new(p.alloc_id(), params));
            }
            let v = chain_mut(p, c)?;
            let rd = v.iter_mut().find(|d| d.id == rack)?;
            let r = rd.rack.as_deref_mut()?;
            r.chains.push(RackChain { id: cid, devices: devs, ..Default::default() });
            let n = r.chains.len() - 1;
            if let FxParams::Rack(rp) = &mut rd.params {
                rp.sel_chain = n as u16;
            }
            Some(n)
        });
        let _ = nsel;
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

    #[test]
    fn sub_chains_resolve_through_racks() {
        use daw_project::devices::{RackChain, RackParams};
        let mut p = Project::default();
        let t = p.add_track(daw_project::TrackKind::Audio);
        let tid = p.tracks[t].id;
        let mut rack = Device::new(10, FxParams::Rack(RackParams::default()));
        rack.rack.as_deref_mut().unwrap().chains.push(RackChain {
            id: 11,
            devices: vec![Device::new(12, FxParams::Utility(Default::default()))],
            ..Default::default()
        });
        p.tracks[t].devices.push(rack);
        let top = ChainRef::Track(tid);
        assert_eq!(chain(&p, ChainRef::Sub(11))[0].id, 12);
        assert_eq!(top_of(&p, ChainRef::Sub(11)), top);
        assert_eq!(chain_of(&p, top, 12), Some(ChainRef::Sub(11)));
        assert_eq!(chain_of(&p, top, 10), Some(top));
        assert_eq!(racks_above(&p, ChainRef::Sub(11)), vec![10]);
        chain_mut(&mut p, ChainRef::Sub(11)).unwrap().push(Device::new(13, FxParams::AlignDelay(Default::default())));
        assert_eq!(chain(&p, ChainRef::Sub(11)).len(), 2);
        let m = MacroMap { macro_idx: 0, device: 12, param: 0, min: 0.2, max: 0.6 };
        assert!((map_value(&m, 127.0) - 0.6).abs() < 1e-6 && (map_value(&m, 0.0) - 0.2).abs() < 1e-6);
    }
}
