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
        // A new sidechain source / tap / switch changes the routing graph.
        let route = |p: &FxParams| (p.sc_active().map(|s| (s.source, s.tap)), p.midi_active().map(|s| s.source));
        if route(&old) != route(&params) {
            self.needs_compile = true;
        }
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

    /// Device `id` anywhere in the project: its chain and a copy.
    pub fn locate_device(&self, id: u64) -> Option<(ChainRef, Device)> {
        for (top, list) in tops(&self.project) {
            if let Some(d) = find_device(list, id) {
                return Some((chain_of(&self.project, top, id)?, d.clone()));
            }
        }
        None
    }

    /// Map LFO `lfo`'s slot `slot` to parameter `k` of device `target`
    /// (one undo step). Mod mode keeps the parameter's value as the base.
    pub fn lfo_set_map(&mut self, lfo: u64, slot: u8, target: u64, k: u8) {
        let Some((c, d)) = self.locate_device(lfo) else { return };
        let FxParams::Lfo(mut l) = d.params else { return };
        let Some(m) = l.maps.get_mut(slot as usize) else { return };
        m.device = target;
        m.param = k;
        self.begin_gesture();
        self.set_device(c, lfo, d.on, FxParams::Lfo(l));
    }

    /// All LFO mappings onto device `dev`: (param, map, LFO output now).
    pub fn lfo_maps_on(&self, dev: u64) -> Vec<(u8, daw_project::devices::LfoMap, f32)> {
        fn walk(list: &[Device], dev: u64, out: &mut Vec<(u8, daw_project::devices::LfoMap, f32)>) {
            for d in list {
                if let FxParams::Lfo(l) = &d.params
                    && d.on
                {
                    for m in l.maps.iter().filter(|m| m.device == dev) {
                        out.push((m.param, *m, daw_project::devices::dev_tap(d.id).get(0)));
                    }
                }
                if let Some(r) = d.rack.as_deref() {
                    for c in &r.chains {
                        walk(&c.devices, dev, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        for (_, list) in tops(&self.project) {
            walk(list, dev, &mut out);
        }
        out
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
        self.group_devices(c, &[id]);
    }

    /// Ctrl+G on several devices of chain `c`: one rack (at the first one's
    /// place) with one chain holding them in chain order.
    pub fn group_devices(&mut self, c: ChainRef, ids: &[u64]) {
        let v = chain(&self.project, c);
        let Some(i0) = v.iter().position(|d| ids.contains(&d.id)) else { return };
        let rid = self.edit(|p| {
            let (rid, cid) = (p.alloc_id(), p.alloc_id());
            let v = chain_mut(p, c)?;
            let mut moved = Vec::new();
            let mut k = 0;
            while k < v.len() {
                if ids.contains(&v[k].id) {
                    moved.push(v.remove(k));
                } else {
                    k += 1;
                }
            }
            let mut rack = Device::new(rid, FxParams::Rack(Default::default()));
            if let Some(r) = rack.rack.as_deref_mut() {
                r.chains.push(RackChain { id: cid, devices: moved, ..Default::default() });
            }
            v.insert(i0.min(v.len()), rack);
            Some(rid)
        });
        if rid.is_some() {
            self.select_only(rid);
        }
    }

    /// Plain selection of one device (or none).
    pub fn select_only(&mut self, id: Option<u64>) {
        self.dev_sel = id;
        self.dev_multi.clear();
        self.dev_multi_owner = id;
        self.dev_anchor = id;
    }

    /// Device `id` is selected (alone or with others).
    pub fn dev_selected(&self, id: u64) -> bool {
        self.dev_sel == Some(id) || (self.dev_multi_owner == self.dev_sel && self.dev_multi.contains(&id))
    }

    /// The selected devices (chain order) and their chain.
    pub fn selected_devices(&self) -> Option<(ChainRef, Vec<u64>)> {
        let (c, _) = self.selected_device()?;
        let v: Vec<u64> = chain(&self.project, c).iter().map(|d| d.id).filter(|&id| self.dev_selected(id)).collect();
        (!v.is_empty()).then_some((c, v))
    }

    /// Click on device `id` of chain `c`: plain = select it alone, Ctrl =
    /// toggle it, Shift = the range from the last clicked device (the same
    /// chain only; like clips).
    pub fn click_device(&mut self, c: ChainRef, id: u64, mods: egui::Modifiers) {
        if self.dev_multi_owner != self.dev_sel {
            self.dev_multi.clear();
        }
        let same = self.selected_device().is_some_and(|(sc, _)| sc == c);
        if mods.command && same {
            if self.dev_selected(id) {
                if self.dev_sel == Some(id) {
                    self.dev_sel = self.dev_multi.pop();
                } else {
                    self.dev_multi.retain(|&x| x != id);
                }
            } else {
                if let Some(s) = self.dev_sel {
                    self.dev_multi.push(s);
                }
                self.dev_sel = Some(id);
                self.dev_anchor = Some(id);
            }
        } else if mods.shift && same {
            let v = chain(&self.project, c);
            let anchor = self.dev_anchor.filter(|a| v.iter().any(|d| d.id == *a)).or(self.dev_sel);
            let (Some(ia), Some(ib)) = (anchor.and_then(|a| v.iter().position(|d| d.id == a)), v.iter().position(|d| d.id == id)) else {
                return self.select_only(Some(id));
            };
            self.dev_multi = v[ia.min(ib)..=ia.max(ib)].iter().map(|d| d.id).filter(|&x| x != id).collect();
            self.dev_sel = Some(id);
        } else {
            return self.select_only(Some(id));
        }
        self.dev_multi_owner = self.dev_sel;
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
        self.select_only(None);
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

    /// Delete several devices of chain `c` (one undo step).
    pub fn delete_devices(&mut self, c: ChainRef, ids: &[u64]) {
        self.edit(|p| {
            if let Some(v) = chain_mut(p, c) {
                v.retain(|d| !ids.contains(&d.id));
            }
        });
        self.select_only(None);
        self.dev_focus = None;
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
            match self.selected_devices() {
                Some((c, ids)) if ids.len() > 1 => self.delete_devices(c, &ids),
                _ => self.delete_device(c, id),
            }
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
        let Some((c, ids)) = self.selected_devices() else { return false };
        if ids.len() == 1 {
            self.duplicate_device(c, ids[0]);
        } else {
            self.duplicate_devices(c, &ids);
        }
        true
    }

    /// Ctrl+D on several devices: copies (in order) after the last one, selected.
    pub fn duplicate_devices(&mut self, c: ChainRef, ids: &[u64]) {
        let new = self.edit(|p| {
            let v = chain(p, c);
            let last = v.iter().rposition(|d| ids.contains(&d.id))?;
            let mut copies: Vec<Device> = v.iter().filter(|d| ids.contains(&d.id)).cloned().collect();
            for d in &mut copies {
                d.reid(&mut || p.alloc_id());
            }
            let nids: Vec<u64> = copies.iter().map(|d| d.id).collect();
            let v = chain_mut(p, c)?;
            for (k, d) in copies.into_iter().enumerate() {
                v.insert(last + 1 + k, d);
            }
            Some(nids)
        });
        if let Some(mut n) = new
            && let Some(last) = n.pop()
        {
            self.dev_sel = Some(last);
            self.dev_multi = n;
            self.dev_multi_owner = Some(last);
        }
    }

    /// Ctrl+G / Ctrl+Shift+G on the selected device(s).
    pub fn group_selected_device(&mut self, ungroup: bool) -> bool {
        let Some((c, ids)) = self.selected_devices() else { return false };
        if ungroup {
            // Every selected rack is replaced by its devices.
            let racks: Vec<u64> =
                ids.iter().copied().filter(|&id| chain(&self.project, c).iter().any(|d| d.id == id && d.rack.is_some())).collect();
            for id in racks {
                self.ungroup_device(c, id);
            }
        } else {
            self.group_devices(c, &ids);
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

    /// Fold / unfold device `id` of chain `c` (one undo step).
    pub fn toggle_device_fold(&mut self, c: ChainRef, id: u64) {
        self.edit(|p| {
            if let Some(d) = chain_mut(p, c).and_then(|v| v.iter_mut().find(|d| d.id == id)) {
                d.folded = !d.folded;
            }
        });
    }

    /// Move a device to the end of track `t`'s chain (`copy`: a copy);
    /// the track gets selected so its chain shows.
    pub fn device_to_track(&mut self, from: ChainRef, id: u64, t: usize, copy: bool) {
        let Some(tid) = self.project.tracks.get(t).map(|x| x.id) else { return };
        let to = ChainRef::Track(tid);
        let at = chain(&self.project, to).len();
        self.move_device(from, id, to, at, copy);
        self.select_track_header(t, crate::arr_ops::SelMode::Single);
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

    fn app_with(n: u64) -> (crate::app::DawApp, ChainRef) {
        let (audio, engine) = crate::audio::start(&crate::audio::AudioOptions { force_null: true, no_input: true, ..Default::default() });
        let mut a = crate::app::DawApp::new(engine, audio, Project::default(), None, false, Default::default(), Default::default());
        let t = a.project.add_track(daw_project::TrackKind::Audio);
        a.selected_track = t;
        let tid = a.project.tracks[t].id;
        for k in 0..n {
            a.project.tracks[t].devices.push(Device::new(100 + k, FxParams::Utility(Default::default())));
        }
        (a, ChainRef::Track(tid))
    }

    #[test]
    fn lfo_maps_parameters_anywhere() {
        use daw_project::devices::{GateParams, LfoParams, RackChain, RackParams};
        let (mut a, c) = app_with(0);
        let mut rack = Device::new(10, FxParams::Rack(RackParams::default()));
        rack.rack.as_deref_mut().unwrap().chains.push(RackChain {
            id: 11,
            devices: vec![Device::new(12, FxParams::Gate(GateParams::default()))],
            ..Default::default()
        });
        let ChainRef::Track(tid) = c else { unreachable!() };
        let t = a.project.tracks.iter().position(|x| x.id == tid).unwrap();
        a.project.tracks[t].devices.push(rack);
        a.project.tracks[t].devices.push(Device::new(20, FxParams::Lfo(LfoParams::default())));
        assert_eq!(a.locate_device(12).map(|x| x.0), Some(ChainRef::Sub(11)));
        a.lfo_set_map(20, 1, 12, daw_project::devices::params::gp::THRESHOLD);
        let FxParams::Lfo(l) = a.locate_device(20).unwrap().1.params else { panic!() };
        assert_eq!((l.maps[1].device, l.maps[1].param), (12, 0));
        let on = a.lfo_maps_on(12);
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].0, 0);
    }

    #[test]
    fn ctrl_and_shift_click_select_like_clips() {
        let (mut a, c) = app_with(4);
        let (ctrl, shift) = (egui::Modifiers::COMMAND, egui::Modifiers::SHIFT);
        a.click_device(c, 100, egui::Modifiers::NONE);
        a.click_device(c, 102, ctrl);
        assert_eq!(a.selected_devices().unwrap().1, vec![100, 102]);
        a.click_device(c, 100, ctrl);
        assert_eq!(a.selected_devices().unwrap().1, vec![102]);
        a.click_device(c, 100, shift);
        assert_eq!(a.selected_devices().unwrap().1, vec![100, 101, 102]);
        // A plain selection elsewhere drops the others.
        a.dev_sel = Some(103);
        assert_eq!(a.selected_devices().unwrap().1, vec![103]);
    }

    #[test]
    fn group_several_and_ungroup_keeps_devices() {
        let (mut a, c) = app_with(4);
        a.click_device(c, 101, egui::Modifiers::NONE);
        a.click_device(c, 103, egui::Modifiers::COMMAND);
        assert!(a.group_selected_device(false));
        let v = chain(&a.project, c);
        assert_eq!(v.len(), 3);
        assert_eq!((v[0].id, v[2].id), (100, 102));
        let rack = v[1].id;
        let inner: Vec<u64> = v[1].rack.as_ref().unwrap().chains[0].devices.iter().map(|d| d.id).collect();
        assert_eq!(inner, vec![101, 103]);
        assert_eq!(a.dev_sel, Some(rack));
        assert!(a.group_selected_device(true));
        let ids: Vec<u64> = chain(&a.project, c).iter().map(|d| d.id).collect();
        assert_eq!(ids, vec![100, 101, 103, 102]);
    }

    #[test]
    fn duplicate_and_delete_several() {
        let (mut a, c) = app_with(3);
        a.click_device(c, 100, egui::Modifiers::NONE);
        a.click_device(c, 101, egui::Modifiers::SHIFT);
        assert!(a.duplicate_selected_device());
        assert_eq!(chain(&a.project, c).len(), 5);
        let (_, sel) = a.selected_devices().unwrap();
        assert_eq!(sel.len(), 2);
        assert!(sel.iter().all(|id| ![100, 101, 102].contains(id)));
        a.delete_devices(c, &sel);
        let ids: Vec<u64> = chain(&a.project, c).iter().map(|d| d.id).collect();
        assert_eq!(ids, vec![100, 101, 102]);
    }
}
