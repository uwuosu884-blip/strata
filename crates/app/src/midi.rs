//! Hardware MIDI input (midir). Every input port is opened; messages are
//! parsed on midir's thread and pushed into the engine's MIDI ring.

use daw_engine::MidiEvent;
use midir::{MidiInput, MidiInputConnection};
use std::sync::{Arc, Mutex};

pub struct MidiSystem {
    producer: Arc<Mutex<rtrb::Producer<MidiEvent>>>,
    connections: Vec<MidiInputConnection<()>>,
    pub ports: Vec<String>,
    pub error: Option<String>,
}

impl MidiSystem {
    pub fn new(producer: rtrb::Producer<MidiEvent>, enabled: bool) -> Self {
        let mut m = Self { producer: Arc::new(Mutex::new(producer)), connections: Vec::new(), ports: Vec::new(), error: None };
        if enabled {
            m.rescan();
        } else {
            m.error = Some("disabled with --no-midi".into());
        }
        m
    }

    /// (Re)connect to all input ports except software "through" ports.
    pub fn rescan(&mut self) {
        self.connections.clear();
        self.ports.clear();
        self.error = None;
        let probe = match MidiInput::new("daw-probe") {
            Ok(m) => m,
            Err(e) => {
                self.error = Some(format!("MIDI unavailable: {e}"));
                return;
            }
        };
        for port in probe.ports() {
            let name = probe.port_name(&port).unwrap_or_else(|_| "MIDI port".into());
            if name.to_ascii_lowercase().contains("through") {
                continue;
            }
            let Ok(input) = MidiInput::new("daw") else { continue };
            let producer = self.producer.clone();
            let res = input.connect(
                &port,
                "daw-in",
                move |_stamp, bytes, _| {
                    if let Some(ev) = MidiEvent::parse(bytes)
                        && let Ok(mut p) = producer.lock()
                    {
                        let _ = p.push(ev);
                    }
                },
                (),
            );
            match res {
                Ok(c) => {
                    self.connections.push(c);
                    self.ports.push(name);
                }
                Err(e) => self.error = Some(format!("{name}: {e}")),
            }
        }
    }
}
