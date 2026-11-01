//! Audio effect devices (a track's device chain).

pub use daw_engine::eq8::{
    EQ_AVG_MAX, EQ_BLOCKS, EQ_FREQ_MAX, EQ_FREQ_MIN, EQ_GAIN_MAX, EQ_OUT_MAX, EQ_Q_MAX, EQ_Q_MIN, EQ_REFRESH_MAX, EQ_REFRESH_MIN,
    EQ_SCALE_MAX,
};
pub use daw_engine::fx::{
    ALIGN_DIST_MAX_M, ALIGN_SAMPLES_MAX, ALIGN_TEMP_MAX_C, ALIGN_TEMP_MIN_C, ALIGN_TIME_MAX_MS, BASS_MONO_MAX_HZ, BASS_MONO_MIN_HZ,
    FEET_PER_M, UTIL_GAIN_MAX_DB, UTIL_GAIN_MIN_DB, UTIL_WIDTH_MAX,
};
pub use daw_engine::params;
pub use daw_engine::rack::{CHAIN_VOL_MAX, CHAIN_VOL_MIN, MACROS, ZONE_MAX};
pub use daw_engine::{AlignDelayParams, ChannelMode, DelayMode, EqBand, EqEightParams, EqFilter, EqMode, FxParams, UtilityParams};
pub use daw_engine::{ChainMix, RackParams, Zone};
pub use daw_engine::{DevTap, dev_tap};
pub use daw_engine::{GateParams, GlueParams, LfoMap, LfoParams, LfoWave, LimMode, LimiterParams, ScEq, ScTap, Sidechain};
use serde::{Deserialize, Serialize};

/// One device in a chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: u64,
    /// Device Activator.
    #[serde(default = "yes")]
    pub on: bool,
    pub params: FxParams,
    /// Audio Effect Rack contents (`FxParams::Rack`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rack: Option<Box<Rack>>,
}

/// Audio Effect Rack: chains, macro mappings and variations.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rack {
    pub chains: Vec<RackChain>,
    pub mappings: Vec<MacroMap>,
    pub variations: Vec<Variation>,
    /// Custom macro names (empty = automatic).
    pub names: Vec<String>,
}

/// A rack chain: mixer, Chain Select zone and devices.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RackChain {
    pub id: u64,
    /// Custom name (empty = named after its devices).
    pub name: String,
    pub mix: ChainMix,
    pub devices: Vec<Device>,
}

impl RackChain {
    /// Shown name: custom, else "Dev A | Dev B", else "Chain".
    pub fn label(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        if self.devices.is_empty() {
            return "Chain".into();
        }
        self.devices.iter().map(|d| d.params.name()).collect::<Vec<_>>().join(" | ")
    }
}

/// Macro `macro_idx` drives parameter `param` of device `device` over the
/// normalised range `min..max` (min > max inverts).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MacroMap {
    pub macro_idx: u8,
    pub device: u64,
    pub param: u8,
    pub min: f32,
    pub max: f32,
}

/// A stored snapshot of the macro values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variation {
    pub name: String,
    pub values: [f32; MACROS],
}

impl Device {
    pub fn new(id: u64, params: FxParams) -> Self {
        let rack = matches!(params, FxParams::Rack(_)).then(Box::default);
        Self { id, on: true, params, rack }
    }

    /// This device's id and every nested device / chain id.
    pub fn ids(&self, out: &mut Vec<u64>) {
        out.push(self.id);
        for c in self.rack.iter().flat_map(|r| &r.chains) {
            out.push(c.id);
            for d in &c.devices {
                d.ids(out);
            }
        }
    }

    /// Give this device and everything inside fresh ids (copies). Mappings
    /// follow their devices.
    pub fn reid(&mut self, alloc: &mut dyn FnMut() -> u64) {
        let mut map = Vec::new();
        self.reid_inner(alloc, &mut map);
        self.remap(&map);
    }

    fn reid_inner(&mut self, alloc: &mut dyn FnMut() -> u64, map: &mut Vec<(u64, u64)>) {
        let n = alloc();
        map.push((self.id, n));
        self.id = n;
        for c in self.rack.iter_mut().flat_map(|r| r.chains.iter_mut()) {
            c.id = alloc();
            for d in &mut c.devices {
                d.reid_inner(alloc, map);
            }
        }
    }

    fn remap(&mut self, map: &[(u64, u64)]) {
        if let Some(r) = self.rack.as_deref_mut() {
            for m in &mut r.mappings {
                if let Some(&(_, n)) = map.iter().find(|(o, _)| *o == m.device) {
                    m.device = n;
                }
            }
            for c in &mut r.chains {
                for d in &mut c.devices {
                    d.remap(map);
                }
            }
        }
    }

    /// Fix ids with `fix` (dedupe on load), depth first.
    pub fn fix_ids(&mut self, fix: &mut dyn FnMut(&mut u64)) {
        fix(&mut self.id);
        for c in self.rack.iter_mut().flat_map(|r| r.chains.iter_mut()) {
            fix(&mut c.id);
            for d in &mut c.devices {
                d.fix_ids(fix);
            }
        }
    }
}

/// Device `id` anywhere in `list` (racks searched).
pub fn find_device(list: &[Device], id: u64) -> Option<&Device> {
    for d in list {
        if d.id == id {
            return Some(d);
        }
        for c in d.rack.iter().flat_map(|r| &r.chains) {
            if let Some(x) = find_device(&c.devices, id) {
                return Some(x);
            }
        }
    }
    None
}

pub fn find_device_mut(list: &mut [Device], id: u64) -> Option<&mut Device> {
    let i = list.iter().position(|d| d.id == id || find_device(std::slice::from_ref(d), id).is_some())?;
    let d = &mut list[i];
    if d.id == id {
        return Some(d);
    }
    d.rack.as_deref_mut()?.chains.iter_mut().find_map(|c| find_device_mut(&mut c.devices, id))
}

/// Rack chain `id` anywhere in `list`.
pub fn find_chain(list: &[Device], id: u64) -> Option<&RackChain> {
    for r in list.iter().filter_map(|d| d.rack.as_deref()) {
        for c in &r.chains {
            if c.id == id {
                return Some(c);
            }
            if let Some(x) = find_chain(&c.devices, id) {
                return Some(x);
            }
        }
    }
    None
}

pub fn find_chain_mut(list: &mut [Device], id: u64) -> Option<&mut RackChain> {
    let i = list.iter().position(|d| find_chain(std::slice::from_ref(d), id).is_some())?;
    let r = list[i].rack.as_deref_mut()?;
    let j = r.chains.iter().position(|c| c.id == id || find_chain(&c.devices, id).is_some())?;
    let c = &mut r.chains[j];
    if c.id == id { Some(c) } else { find_chain_mut(&mut c.devices, id) }
}

/// The rack device that owns chain `id`.
pub fn chain_owner(list: &[Device], id: u64) -> Option<&Device> {
    for d in list {
        if let Some(r) = d.rack.as_deref() {
            if r.chains.iter().any(|c| c.id == id) {
                return Some(d);
            }
            for c in &r.chains {
                if let Some(x) = chain_owner(&c.devices, id) {
                    return Some(x);
                }
            }
        }
    }
    None
}

/// The rack device whose chain directly contains device `id` (None = top level).
pub fn device_parent(list: &[Device], id: u64) -> Option<(&Device, &RackChain)> {
    for d in list {
        if let Some(r) = d.rack.as_deref() {
            for c in &r.chains {
                if c.devices.iter().any(|x| x.id == id) {
                    return Some((d, c));
                }
                if let Some(x) = device_parent(&c.devices, id) {
                    return Some(x);
                }
            }
        }
    }
    None
}

fn yes() -> bool {
    true
}

/// Audio effects offered by the browser (name, default parameters).
pub fn audio_effects() -> [(&'static str, FxParams); 8] {
    [
        ("Audio Effect Rack", FxParams::Rack(RackParams::default())),
        ("Align Delay", FxParams::AlignDelay(AlignDelayParams::default())),
        ("EQ Eight", FxParams::EqEight(EqEightParams::default())),
        ("Gate", FxParams::Gate(GateParams::default())),
        ("Glue Compressor", FxParams::Glue(GlueParams::default())),
        ("LFO", FxParams::Lfo(LfoParams::default())),
        ("Limiter", FxParams::Limiter(LimiterParams::default())),
        ("Utility", FxParams::Utility(UtilityParams::default())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rack_with(id: u64, chain: u64, devs: Vec<Device>) -> Device {
        let mut r = Device::new(id, FxParams::Rack(RackParams::default()));
        r.rack.as_deref_mut().unwrap().chains.push(RackChain { id: chain, devices: devs, ..Default::default() });
        r
    }

    #[test]
    fn nested_lookup_reid_and_mappings() {
        let inner = rack_with(3, 4, vec![Device::new(5, FxParams::Utility(Default::default()))]);
        let mut outer = rack_with(1, 2, vec![inner]);
        outer.rack.as_deref_mut().unwrap().mappings.push(MacroMap { macro_idx: 0, device: 5, param: 0, min: 0.0, max: 1.0 });
        let list = vec![outer.clone()];
        assert_eq!(find_device(&list, 5).map(|d| d.id), Some(5));
        assert_eq!(find_chain(&list, 4).map(|c| c.id), Some(4));
        assert_eq!(chain_owner(&list, 4).map(|d| d.id), Some(3));
        assert_eq!(device_parent(&list, 3).map(|(d, c)| (d.id, c.id)), Some((1, 2)));
        let mut ids = Vec::new();
        outer.ids(&mut ids);
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
        let mut n = 100;
        outer.reid(&mut || {
            n += 1;
            n
        });
        let mut ids = Vec::new();
        outer.ids(&mut ids);
        assert!(ids.iter().all(|&i| i > 100));
        // The mapping follows the copied Utility.
        let m = outer.rack.as_ref().unwrap().mappings[0];
        assert!(find_device(std::slice::from_ref(&outer), m.device).is_some_and(|d| matches!(d.params, FxParams::Utility(_))));
        let mut l2 = vec![outer];
        assert!(find_chain_mut(&mut l2, ids[3]).is_some());
        assert!(find_device_mut(&mut l2, ids[4]).is_some());
    }

    #[test]
    fn chain_labels_and_serde_round_trip() {
        let r = rack_with(
            1,
            2,
            vec![Device::new(3, FxParams::AlignDelay(Default::default())), Device::new(4, FxParams::Utility(Default::default()))],
        );
        assert_eq!(r.rack.as_ref().unwrap().chains[0].label(), "Align Delay | Utility");
        let s = ron::to_string(&r).unwrap();
        let back: Device = ron::from_str(&s).unwrap();
        assert_eq!(back, r);
        // Plain devices don't write a rack field.
        assert!(!ron::to_string(&Device::new(9, FxParams::Utility(Default::default()))).unwrap().contains("rack"));
    }
}
