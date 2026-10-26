//! Hardware MIDI (midir). Input ports whose "Track" switch is on are opened;
//! messages are tagged with their port index + channel on midir's thread and
//! pushed into the engine's MIDI ring. Output ports with "Track" on receive
//! what the engine sends (a small thread drains the engine's MIDI-out ring).

use crate::settings::Settings;
use daw_engine::{MidiEvent, MidiInput as EngineMidi, MidiOutput as EngineMidiOut};
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Ports tracked for per-port/per-channel activity meters.
pub const MAX_PORTS: usize = 16;

/// Activity counters (LEDs / chooser meters). Times are ms since `epoch`.
pub struct Activity {
    epoch: Instant,
    /// Any Track input / output message.
    pub track_in: AtomicU64,
    pub track_out: AtomicU64,
    /// Last message time per engine input port and channel.
    ports: [[AtomicU64; 16]; MAX_PORTS],
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            track_in: AtomicU64::new(0),
            track_out: AtomicU64::new(0),
            ports: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
        }
    }
}

impl Activity {
    fn now(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64 + 1
    }

    pub fn note_in(&self, port: usize, channel: u8) {
        let t = self.now();
        self.track_in.store(t, Relaxed);
        if let Some(p) = self.ports.get(port) {
            p[(channel & 15) as usize].store(t, Relaxed);
        }
    }

    pub fn note_out(&self) {
        self.track_out.store(self.now(), Relaxed);
    }

    /// Seconds since the last activity of `stamp` (∞ if never).
    pub fn age(&self, stamp: &AtomicU64) -> f32 {
        match stamp.load(Relaxed) {
            0 => f32::INFINITY,
            t => self.now().saturating_sub(t) as f32 / 1000.0,
        }
    }

    /// Seconds since activity on a port (`channel` None = any channel).
    pub fn port_age(&self, port: usize, channel: Option<u8>) -> f32 {
        let Some(p) = self.ports.get(port) else { return f32::INFINITY };
        match channel {
            Some(c) => self.age(&p[(c & 15) as usize]),
            None => p.iter().map(|s| self.age(s)).fold(f32::INFINITY, f32::min),
        }
    }
}

/// An available port and whether its "Track" switch is on / it opened.
#[derive(Debug, Clone, PartialEq)]
pub struct PortInfo {
    pub name: String,
    pub track: bool,
    pub open: bool,
}

pub struct MidiSystem {
    producer: Arc<Mutex<rtrb::Producer<EngineMidi>>>,
    in_conns: Vec<MidiInputConnection<()>>,
    outs: Arc<Mutex<Vec<Option<MidiOutputConnection>>>>,
    out_thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    /// Every port found, in system order.
    pub inputs: Vec<PortInfo>,
    pub outputs: Vec<PortInfo>,
    pub activity: Arc<Activity>,
    pub error: Option<String>,
    enabled: bool,
    pub dropped: Arc<AtomicU32>,
}

impl MidiSystem {
    pub fn new(
        producer: rtrb::Producer<EngineMidi>,
        out: Option<rtrb::Consumer<EngineMidiOut>>,
        enabled: bool,
        settings: &Settings,
    ) -> Self {
        let outs: Arc<Mutex<Vec<Option<MidiOutputConnection>>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(Activity::default());
        let out_thread = out.map(|mut rx| {
            let (outs, stop, act) = (outs.clone(), stop.clone(), activity.clone());
            std::thread::Builder::new()
                .name("daw-midi-out".into())
                .spawn(move || {
                    while !stop.load(Relaxed) {
                        let mut sent = false;
                        while let Ok(m) = rx.pop() {
                            if let Ok(mut o) = outs.lock()
                                && let Some(Some(c)) = o.get_mut(m.port as usize)
                            {
                                let len = if m.bytes[0] & 0xF0 == 0xC0 || m.bytes[0] & 0xF0 == 0xD0 { 2 } else { 3 };
                                let _ = c.send(&m.bytes[..len]);
                                sent = true;
                            }
                        }
                        if sent {
                            act.note_out();
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                })
                .expect("spawn midi out")
        });
        let mut m = Self {
            producer: Arc::new(Mutex::new(producer)),
            in_conns: Vec::new(),
            outs,
            out_thread,
            stop,
            inputs: Vec::new(),
            outputs: Vec::new(),
            activity,
            error: None,
            enabled,
            dropped: Arc::default(),
        };
        if enabled {
            m.rescan(settings);
        } else {
            m.error = Some("disabled with --no-midi".into());
        }
        m
    }

    /// Engine port names (open ports with "Track" on), index = engine port.
    pub fn io_map(&self) -> daw_core::IoMap {
        daw_core::IoMap {
            midi_in: self.inputs.iter().filter(|p| p.open).map(|p| p.name.clone()).collect(),
            midi_out: self.outputs.iter().filter(|p| p.open).map(|p| p.name.clone()).collect(),
        }
    }

    /// (Re)list ports and connect the ones whose Track switch is on.
    pub fn rescan(&mut self, settings: &Settings) {
        if !self.enabled {
            return;
        }
        self.in_conns.clear();
        self.inputs.clear();
        self.outputs.clear();
        self.error = None;
        match MidiInput::new("daw-probe") {
            Ok(probe) => {
                for port in probe.ports() {
                    let name = probe.port_name(&port).unwrap_or_else(|_| "MIDI port".into());
                    let track = settings.midi_in_on(&name);
                    let mut info = PortInfo { name: name.clone(), track, open: false };
                    if track {
                        let index = self.in_conns.len();
                        match self.connect_input(&port, index) {
                            Ok(c) => {
                                self.in_conns.push(c);
                                info.open = true;
                            }
                            Err(e) => self.error = Some(format!("{name}: {e}")),
                        }
                    }
                    self.inputs.push(info);
                }
            }
            Err(e) => self.error = Some(format!("MIDI unavailable: {e}")),
        }
        let mut conns = Vec::new();
        if let Ok(probe) = MidiOutput::new("daw-probe-out") {
            for port in probe.ports() {
                let name = probe.port_name(&port).unwrap_or_else(|_| "MIDI port".into());
                let track = settings.midi_out_on(&name);
                let mut info = PortInfo { name: name.clone(), track, open: false };
                if track {
                    match MidiOutput::new("daw")
                        .map_err(|e| e.to_string())
                        .and_then(|o| o.connect(&port, "daw-out").map_err(|e| e.to_string()))
                    {
                        Ok(c) => {
                            conns.push(Some(c));
                            info.open = true;
                        }
                        Err(e) => self.error = Some(format!("{name}: {e}")),
                    }
                }
                self.outputs.push(info);
            }
        }
        if let Ok(mut o) = self.outs.lock() {
            *o = conns;
        }
    }

    fn connect_input(&self, port: &midir::MidiInputPort, index: usize) -> Result<MidiInputConnection<()>, String> {
        let input = MidiInput::new("daw").map_err(|e| e.to_string())?;
        let (producer, act, dropped) = (self.producer.clone(), self.activity.clone(), self.dropped.clone());
        let port_idx = index.min(daw_engine::MIDI_NO_PORT as usize - 1) as u8;
        input
            .connect(
                port,
                "daw-in",
                move |_stamp, bytes, _| {
                    if let Some(ev) = MidiEvent::parse(bytes) {
                        let channel = MidiEvent::channel_of(bytes);
                        act.note_in(port_idx as usize, channel);
                        if let Ok(mut p) = producer.lock()
                            && p.push(EngineMidi { port: port_idx, channel, event: ev }).is_err()
                        {
                            dropped.fetch_add(1, Relaxed);
                        }
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())
    }
}

impl Drop for MidiSystem {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.out_thread.take() {
            let _ = t.join();
        }
    }
}
