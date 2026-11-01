//! Audio Effect Rack (Live 12 manual ch. 25): parallel chains that all get
//! the rack input, run their devices serially and are summed. Chains are
//! gated by Chain Select zones (fades attenuate the chain output), their
//! activator / solo, and mixed with volume and pan.

use crate::fx::{FxSlot, run_chain};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_4, SQRT_2};

/// Macro controls a rack can show.
pub const MACROS: usize = 16;
/// Chain Select editor range 0..=127.
pub const ZONE_MAX: u8 = 127;
/// Chain volume range (dB); −inf below the minimum.
pub const CHAIN_VOL_MIN: f32 = -70.0;
pub const CHAIN_VOL_MAX: f32 = 6.0;
/// Rack processing sub-block (fixed scratch, any engine block size).
const SUB: usize = 256;

/// Rack-level parameters: macros, chain selector and the view column state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RackParams {
    /// Macro values 0..=127.
    pub macros: [f32; MACROS],
    /// Visible macros (1..=16, the +/− buttons).
    pub visible: u8,
    /// Chain selector 0..=127.
    pub selector: f32,
    pub show_macros: bool,
    pub show_variations: bool,
    pub show_chains: bool,
    pub show_devices: bool,
    /// Chain list: Chain Select zone editor shown ("Chain") or hidden ("Hide").
    pub zones: bool,
    pub auto_select: bool,
    /// Selected chain (index); its devices fill the Devices view.
    pub sel_chain: u16,
    /// Macros excluded from Rand / from variations (bit per macro).
    pub no_rand: u16,
    pub no_var: u16,
    /// Map mode (UI only).
    #[serde(skip)]
    pub map: bool,
    /// Map mode: macro whose Map button is armed (UI only).
    #[serde(skip)]
    pub map_sel: Option<u8>,
}

impl Default for RackParams {
    fn default() -> Self {
        Self {
            macros: [0.0; MACROS],
            visible: 8,
            selector: 0.0,
            show_macros: false,
            show_variations: false,
            show_chains: false,
            show_devices: true,
            zones: false,
            auto_select: false,
            sel_chain: 0,
            no_rand: 0,
            no_var: 0,
            map: false,
            map_sel: None,
        }
    }
}

/// A Chain Select zone: chain is active for `lo..=hi`; the output fades in
/// over `lo..=fade_lo` and out over `fade_hi..=hi`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Zone {
    pub lo: u8,
    pub hi: u8,
    pub fade_lo: u8,
    pub fade_hi: u8,
}

impl Default for Zone {
    /// Live: length 1 at value 0.
    fn default() -> Self {
        Self { lo: 0, hi: 0, fade_lo: 0, fade_hi: 0 }
    }
}

impl Zone {
    /// Keep `lo <= fade_lo <= fade_hi <= hi <= 127`.
    pub fn normalized(mut self) -> Self {
        self.hi = self.hi.min(ZONE_MAX);
        self.lo = self.lo.min(self.hi);
        self.fade_lo = self.fade_lo.clamp(self.lo, self.hi);
        self.fade_hi = self.fade_hi.clamp(self.fade_lo, self.hi);
        self
    }

    /// Selector inside the zone (the chain receives input).
    pub fn contains(&self, sel: u8) -> bool {
        (self.lo..=self.hi).contains(&sel)
    }

    /// Output weight at selector `sel` (0 outside, fades linear).
    pub fn weight(&self, sel: u8) -> f32 {
        let z = self.normalized();
        if !z.contains(sel) {
            return 0.0;
        }
        let fin = if sel < z.fade_lo { (sel - z.lo + 1) as f32 / (z.fade_lo - z.lo + 1) as f32 } else { 1.0 };
        let fout = if sel > z.fade_hi { (z.hi - sel + 1) as f32 / (z.hi - z.fade_hi + 1) as f32 } else { 1.0 };
        fin.min(fout)
    }
}

/// Chain mixer state (chain list controls).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChainMix {
    pub on: bool,
    pub solo: bool,
    /// dB; at or below `CHAIN_VOL_MIN` = −inf.
    pub volume: f32,
    /// −1..1 (Live 50L..50R).
    pub pan: f32,
    pub zone: Zone,
}

impl Default for ChainMix {
    fn default() -> Self {
        Self { on: true, solo: false, volume: 0.0, pan: 0.0, zone: Zone::default() }
    }
}

impl ChainMix {
    /// Target (left, right) gains; `soloing` = some chain of the rack is soloed.
    pub fn gains(&self, sel: u8, soloing: bool) -> (f32, f32) {
        if !self.on || (soloing && !self.solo) {
            return (0.0, 0.0);
        }
        let w = self.zone.weight(sel);
        let g = if self.volume <= CHAIN_VOL_MIN { 0.0 } else { 10f32.powf(self.volume / 20.0) } * w;
        let a = (self.pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        (g * a.cos() * SQRT_2, g * a.sin() * SQRT_2)
    }
}

/// One chain on the audio thread.
#[derive(Debug, Clone)]
pub struct ChainDsp {
    /// Project chain id.
    pub id: u64,
    pub mix: ChainMix,
    pub devices: Vec<FxSlot>,
    /// Gains applied at the end of the last sub-block (ramp start).
    cur: Option<(f32, f32)>,
}

impl ChainDsp {
    pub fn new(id: u64, mix: ChainMix, devices: Vec<FxSlot>) -> Self {
        Self { id, mix, devices, cur: None }
    }
}

/// Rack DSP: chains plus preallocated scratch.
#[derive(Debug, Clone)]
pub struct RackDsp {
    pub chains: Vec<ChainDsp>,
    inp: Box<[[f32; SUB]; 2]>,
    acc: Box<[[f32; SUB]; 2]>,
    work: Box<[[f32; SUB]; 2]>,
}

impl RackDsp {
    /// Control thread (allocates).
    pub fn new(chains: Vec<ChainDsp>) -> Self {
        Self { chains, inp: Box::new([[0.0; SUB]; 2]), acc: Box::new([[0.0; SUB]; 2]), work: Box::new([[0.0; SUB]; 2]) }
    }

    /// Process `l`/`r` in place. No chains = the input passes unchanged.
    pub fn process(&mut self, p: &RackParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        if self.chains.is_empty() {
            return;
        }
        let sel = p.selector.round().clamp(0.0, ZONE_MAX as f32) as u8;
        let soloing = self.chains.iter().any(|c| c.mix.solo);
        let n = l.len().min(r.len());
        let mut off = 0;
        while off < n {
            let m = (n - off).min(SUB);
            let Self { chains, inp, acc, work } = self;
            inp[0][..m].copy_from_slice(&l[off..off + m]);
            inp[1][..m].copy_from_slice(&r[off..off + m]);
            acc[0][..m].fill(0.0);
            acc[1][..m].fill(0.0);
            for c in chains.iter_mut() {
                let (tl, tr) = c.mix.gains(sel, soloing);
                let (sl, sr0) = c.cur.unwrap_or((tl, tr));
                c.cur = Some((tl, tr));
                // Outside its zone the chain gets no input (tails ring out).
                let fed = c.mix.on && c.mix.zone.contains(sel);
                if fed {
                    work[0][..m].copy_from_slice(&inp[0][..m]);
                    work[1][..m].copy_from_slice(&inp[1][..m]);
                } else {
                    work[0][..m].fill(0.0);
                    work[1][..m].fill(0.0);
                }
                if !c.devices.is_empty() {
                    let (wl, wr) = work.split_at_mut(1);
                    run_chain(&mut c.devices, &mut wl[0][..m], &mut wr[0][..m], sr, &[]);
                }
                if sl == 0.0 && tl == 0.0 && sr0 == 0.0 && tr == 0.0 {
                    continue;
                }
                let (dl, dr) = ((tl - sl) / m as f32, (tr - sr0) / m as f32);
                for i in 0..m {
                    let k = i as f32;
                    acc[0][i] += work[0][i] * (sl + dl * k);
                    acc[1][i] += work[1][i] * (sr0 + dr * k);
                }
            }
            l[off..off + m].copy_from_slice(&acc[0][..m]);
            r[off..off + m].copy_from_slice(&acc[1][..m]);
            off += m;
        }
    }

    /// Find a chain anywhere below this rack (nested racks included).
    pub fn chain_mut(&mut self, id: u64) -> Option<&mut ChainDsp> {
        if let Some(i) = self.chains.iter().position(|c| c.id == id) {
            return Some(&mut self.chains[i]);
        }
        for c in self.chains.iter_mut() {
            for d in c.devices.iter_mut() {
                if let Some(r) = d.rack.as_deref_mut()
                    && let Some(x) = r.chain_mut(id)
                {
                    return Some(x);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{FxParams, UtilityParams};

    fn rack(chains: Vec<ChainDsp>) -> FxSlot {
        let mut s = FxSlot::new(1, true, FxParams::Rack(RackParams::default()));
        s.rack = Some(Box::new(RackDsp::new(chains)));
        s
    }

    #[test]
    fn zone_weights_and_fades() {
        let z = Zone { lo: 10, hi: 20, fade_lo: 13, fade_hi: 18 };
        assert_eq!(z.weight(9), 0.0);
        assert_eq!(z.weight(21), 0.0);
        assert_eq!(z.weight(15), 1.0);
        assert!(z.weight(10) > 0.0 && z.weight(10) < z.weight(12));
        assert!(z.weight(20) > 0.0 && z.weight(20) < z.weight(19));
        assert_eq!(Zone::default().weight(0), 1.0);
        assert_eq!(Zone::default().weight(1), 0.0);
    }

    #[test]
    fn chains_sum_in_parallel() {
        // Two empty chains = input twice; a muted Utility chain adds nothing.
        let mute = FxSlot::new(3, true, FxParams::Utility(UtilityParams { mute: true, ..Default::default() }));
        let mut s = vec![rack(vec![
            ChainDsp::new(10, ChainMix::default(), vec![]),
            ChainDsp::new(11, ChainMix::default(), vec![]),
            ChainDsp::new(12, ChainMix::default(), vec![mute]),
        ])];
        let mut l = vec![0.25f32; 600];
        let mut r = vec![-0.25f32; 600];
        run_chain(&mut s, &mut l, &mut r, 48000.0, &[]);
        assert!(l.iter().all(|x| (x - 0.5).abs() < 1e-5), "{}", l[0]);
        assert!(r.iter().all(|x| (x + 0.5).abs() < 1e-5));
    }

    #[test]
    fn solo_activator_and_selector_gate_chains() {
        let solo = ChainMix { solo: true, ..Default::default() };
        let off = ChainMix { on: false, ..Default::default() };
        let far = ChainMix { zone: Zone { lo: 64, hi: 127, fade_lo: 64, fade_hi: 127 }, ..Default::default() };
        let mut s = vec![rack(vec![
            ChainDsp::new(10, solo, vec![]),
            ChainDsp::new(11, ChainMix::default(), vec![]),
            ChainDsp::new(12, off, vec![]),
            ChainDsp::new(13, far, vec![]),
        ])];
        let mut l = vec![1.0f32; 64];
        let mut r = vec![1.0f32; 64];
        run_chain(&mut s, &mut l, &mut r, 48000.0, &[]);
        assert!((l[10] - 1.0).abs() < 1e-5, "only the soloed chain: {}", l[10]);
        // Selector 64: only the far chain (solo off).
        if let FxParams::Rack(p) = &mut s[0].params {
            p.selector = 64.0;
        }
        if let Some(rk) = s[0].rack.as_deref_mut() {
            rk.chains[0].mix.solo = false;
            rk.chains[0].cur = None;
            rk.chains[1].cur = None;
            rk.chains[3].cur = None;
        }
        let mut l = vec![1.0f32; 64];
        let mut r = vec![1.0f32; 64];
        run_chain(&mut s, &mut l, &mut r, 48000.0, &[]);
        assert!((l[10] - 1.0).abs() < 1e-5, "{}", l[10]);
    }

    #[test]
    fn empty_rack_passes_and_pan_law_is_unity_at_centre() {
        let mut s = vec![rack(vec![])];
        let mut l = vec![0.3f32; 10];
        let mut r = vec![0.3f32; 10];
        run_chain(&mut s, &mut l, &mut r, 48000.0, &[]);
        assert_eq!(l[5], 0.3);
        let (a, b) = ChainMix::default().gains(0, false);
        assert!((a - 1.0).abs() < 1e-6 && (b - 1.0).abs() < 1e-6);
        let (a, b) = ChainMix { pan: -1.0, ..Default::default() }.gains(0, false);
        assert!(b.abs() < 1e-6 && (a - SQRT_2).abs() < 1e-5);
    }
}
