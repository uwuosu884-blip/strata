//! App preferences (not part of the project): audio device, per-device
//! channel configuration and names, MIDI port switches. Saved as RON in the
//! user's config directory (`DAW_CONFIG_DIR` overrides it).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Which device channels are offered in the I/O choosers (Live's "Channel
/// Configuration"): every channel as mono and/or every pair as stereo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ChannelConfig {
    /// Disabled entries (everything is enabled by default): "m3" = mono
    /// channel index 3, "s2" = stereo pair starting at index 2.
    pub off: Vec<String>,
    /// Custom names by the same keys.
    pub names: HashMap<String, String>,
}

/// One chooser entry: first channel (0-based) and mono/stereo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChanEntry {
    pub ch: u16,
    pub stereo: bool,
}

impl ChanEntry {
    pub fn key(&self) -> String {
        format!("{}{}", if self.stereo { 's' } else { 'm' }, self.ch)
    }

    /// "1/2" or "3" (1-based like Live).
    pub fn number(&self) -> String {
        if self.stereo { format!("{}/{}", self.ch + 1, self.ch + 2) } else { format!("{}", self.ch + 1) }
    }
}

impl ChannelConfig {
    /// Every possible entry for a device with `channels` channels, Live order
    /// (per pair: the two monos, then the stereo pair).
    pub fn all_entries(channels: u16) -> Vec<ChanEntry> {
        let mut v = Vec::new();
        let mut c = 0;
        while c < channels {
            v.push(ChanEntry { ch: c, stereo: false });
            if c + 1 < channels {
                v.push(ChanEntry { ch: c + 1, stereo: false });
                v.push(ChanEntry { ch: c, stereo: true });
            }
            c += 2;
        }
        v
    }

    pub fn enabled(&self, e: ChanEntry) -> bool {
        !self.off.contains(&e.key())
    }

    pub fn set_enabled(&mut self, e: ChanEntry, on: bool) {
        let k = e.key();
        self.off.retain(|x| *x != k);
        if !on {
            self.off.push(k);
        }
    }

    pub fn entries(&self, channels: u16) -> Vec<ChanEntry> {
        Self::all_entries(channels).into_iter().filter(|e| self.enabled(*e)).collect()
    }

    /// Display label: number plus custom name.
    pub fn label(&self, e: ChanEntry) -> String {
        match self.names.get(&e.key()).filter(|n| !n.trim().is_empty()) {
            Some(n) => format!("{} {}", e.number(), n.trim()),
            None => e.number(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub host: Option<String>,
    pub output_device: Option<String>,
    pub input_device: Option<String>,
    pub no_input: bool,
    pub buffer_frames: Option<u32>,
    /// Preferences sample rate (None = device default).
    pub sample_rate: Option<u32>,
    /// Top-bar output (monitoring) level in dB; never part of renders.
    pub output_gain_db: f32,
    /// Channel configuration per device ("in:<name>" / "out:<name>").
    pub channels: HashMap<String, ChannelConfig>,
    /// MIDI "Track" switches that differ from the default (inputs on except
    /// "through" ports, outputs off).
    pub midi_in: HashMap<String, bool>,
    pub midi_out: HashMap<String, bool>,
    /// Settings > Theme & Colors.
    pub theme: crate::ui::theme::ThemeSettings,
    /// Display & Input > Zoom Display in % (0 = 100).
    pub zoom_pct: u16,
    /// Browser: collections, columns, sidebar width.
    pub browser: BrowserPrefs,
}

/// Saved browser state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserPrefs {
    /// Item key → collection bits (bit i = collection i: Favorites, Orange, …, Gray).
    pub collections: std::collections::BTreeMap<String, u8>,
    /// Collections hidden from the sidebar (bits).
    pub hidden: u8,
    /// Extra list columns (bits: Date Modified, Size, Type, Rank, Place).
    pub columns: u8,
    pub show_extensions: bool,
    /// Sidebar width in px (below 60 = narrow icon-only sidebar).
    pub sidebar_w: f32,
}

impl Default for BrowserPrefs {
    fn default() -> Self {
        Self { collections: Default::default(), hidden: 0, columns: 0, show_extensions: true, sidebar_w: 27.0 }
    }
}

impl Settings {
    /// Zoom Display in % (50..=200).
    pub fn zoom(&self) -> u16 {
        if self.zoom_pct == 0 { 100 } else { self.zoom_pct.clamp(50, 200) }
    }

    pub fn path() -> Option<PathBuf> {
        if let Some(d) = std::env::var_os("DAW_CONFIG_DIR") {
            return Some(PathBuf::from(d).join("settings.ron"));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            home.map(|h| h.join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".config")))
        };
        base.map(|b| b.join("daw").join("settings.ron"))
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| ron::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config directory")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    pub fn midi_in_on(&self, name: &str) -> bool {
        *self.midi_in.get(name).unwrap_or(&!name.to_ascii_lowercase().contains("through"))
    }

    pub fn midi_out_on(&self, name: &str) -> bool {
        *self.midi_out.get(name).unwrap_or(&false)
    }

    pub fn channel_config(&self, input: bool, device: &str) -> ChannelConfig {
        self.channels.get(&Self::device_key(input, device)).cloned().unwrap_or_default()
    }

    pub fn device_key(input: bool, device: &str) -> String {
        format!("{}:{device}", if input { "in" } else { "out" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_entries_and_labels() {
        let mut c = ChannelConfig::default();
        let all = ChannelConfig::all_entries(3);
        assert_eq!(all.iter().map(|e| e.number()).collect::<Vec<_>>(), vec!["1", "2", "1/2", "3"]);
        c.set_enabled(ChanEntry { ch: 1, stereo: false }, false);
        c.names.insert("s0".into(), "Guitar".into());
        assert_eq!(c.entries(3).iter().map(|e| c.label(*e)).collect::<Vec<_>>(), vec!["1", "1/2 Guitar", "3"]);
        c.set_enabled(ChanEntry { ch: 1, stereo: false }, true);
        assert_eq!(c.entries(3).len(), 4);
    }

    #[test]
    fn settings_round_trip_and_midi_defaults() {
        let mut s = Settings { host: Some("ALSA".into()), buffer_frames: Some(128), ..Default::default() };
        s.channels.insert(Settings::device_key(true, "USB"), ChannelConfig { off: vec!["m0".into()], names: HashMap::new() });
        s.midi_out.insert("Synth".into(), true);
        let text = ron::to_string(&s).unwrap();
        let back: Settings = ron::from_str(&text).unwrap();
        assert_eq!(back, s);
        assert!(s.midi_in_on("Keys") && !s.midi_in_on("Midi Through Port-0"));
        assert!(s.midi_out_on("Synth") && !s.midi_out_on("Other"));
        assert!(!s.channel_config(true, "USB").enabled(ChanEntry { ch: 0, stereo: false }));
    }
}
