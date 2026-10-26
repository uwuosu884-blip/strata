//! Recorder thread: drains the engine's record rings continuously (so they
//! never overflow) and assembles finished takes for the GUI to turn into clips.

use daw_engine::{RecordEvent, RecorderPorts};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedNote {
    pub track: u16,
    pub pitch: u8,
    pub velocity: u8,
    pub start_beat: f64,
    pub end_beat: f64,
    /// Position inside the playing session clip at note-on (overdub), if any.
    pub clip_pos: Option<f64>,
}

/// One armed audio track's recording.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TakeLane {
    /// Runtime track (engine slot).
    pub track: u16,
    /// Recorded from a mono input (L == R): store as a mono sample.
    pub mono: bool,
    /// Interleaved stereo.
    pub audio: Vec<f32>,
}

impl TakeLane {
    /// Drop the first `frames` (latency) so audio lines up with the grid.
    pub fn compensate_latency(&mut self, frames: usize) {
        let n = (frames * 2).min(self.audio.len());
        self.audio.drain(..n);
        self.audio.extend(std::iter::repeat_n(0.0, n));
    }

    pub fn frames(&self) -> usize {
        self.audio.len() / 2
    }
}

#[derive(Debug, Clone, Default)]
pub struct Take {
    pub start_beat: f64,
    pub end_beat: f64,
    /// Audio per armed audio track.
    pub lanes: Vec<TakeLane>,
    pub notes: Vec<RecordedNote>,
}

impl Take {
    pub fn lane(&self, track: u16) -> Option<&TakeLane> {
        self.lanes.iter().find(|l| l.track == track)
    }
}

/// A take in progress: start beat/position and lane layout.
#[derive(Clone)]
struct Open {
    beat: f64,
    pos: u64,
    lanes: Vec<u16>,
    mono: u16,
}

#[derive(Default)]
struct State {
    /// Sample index of `audio[0]` in the engine's record stream.
    base: u64,
    audio: Vec<f32>,
    current: Option<Open>,
    open: HashMap<(u16, u8), (u8, f64, Option<f64>)>,
    notes: Vec<RecordedNote>,
    /// Stopped but still waiting for audio up to the end position.
    finishing: Vec<(Open, f64, u64, Vec<RecordedNote>)>,
    done: Vec<Take>,
    recording: bool,
    /// End position of the last take; audio after it belongs to the next take.
    last_stop: u64,
}

impl State {
    fn on_event(&mut self, ev: RecordEvent) {
        match ev {
            RecordEvent::Started { beat, pos, lanes, n_lanes, mono } => {
                self.current = Some(Open { beat, pos, lanes: lanes[..n_lanes as usize].to_vec(), mono });
                self.notes.clear();
                self.open.clear();
                self.recording = true;
            }
            RecordEvent::Stopped { beat, pos } => {
                if let Some(open) = self.current.take() {
                    // Close hanging notes at the stop position.
                    for ((track, pitch), (velocity, s, p)) in self.open.drain() {
                        self.notes.push(RecordedNote { track, pitch, velocity, start_beat: s, end_beat: beat, clip_pos: p });
                    }
                    let mut notes = std::mem::take(&mut self.notes);
                    notes.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
                    self.finishing.push((open, beat, pos, notes));
                    self.last_stop = self.last_stop.max(pos);
                }
                self.recording = false;
            }
            RecordEvent::NoteOn { track, pitch, velocity, beat, clip_pos } => {
                if self.current.is_some()
                    && let Some((v, s, p)) = self.open.insert((track, pitch), (velocity, beat, clip_pos))
                {
                    // Retrigger without note-off: close the previous one.
                    self.notes.push(RecordedNote { track, pitch, velocity: v, start_beat: s, end_beat: beat, clip_pos: p });
                }
            }
            RecordEvent::NoteOff { track, pitch, beat, .. } => {
                if let Some((velocity, s, pos)) = self.open.remove(&(track, pitch)) {
                    self.notes.push(RecordedNote { track, pitch, velocity, start_beat: s, end_beat: beat.max(s), clip_pos: pos });
                }
            }
        }
    }

    fn finish_ready(&mut self) {
        let have = self.base + self.audio.len() as u64;
        let mut i = 0;
        while i < self.finishing.len() {
            if self.finishing[i].2 <= have {
                let (open, end_beat, p1, notes) = self.finishing.remove(i);
                let a = (open.pos.saturating_sub(self.base) as usize).min(self.audio.len());
                let b = (p1.saturating_sub(self.base) as usize).min(self.audio.len());
                let raw = &self.audio[a..b.max(a)];
                // Per frame: (L, R) of each lane.
                let width = open.lanes.len() * 2;
                let lanes = open
                    .lanes
                    .iter()
                    .enumerate()
                    .map(|(k, &track)| TakeLane {
                        track,
                        mono: open.mono & (1 << k) != 0,
                        audio: raw.chunks_exact(width).flat_map(|f| [f[2 * k], f[2 * k + 1]]).collect(),
                    })
                    .collect();
                self.done.push(Take { start_beat: open.beat, end_beat, lanes, notes });
            } else {
                i += 1;
            }
        }
        // Drop audio nobody needs any more.
        let keep_from =
            self.finishing.iter().map(|f| f.0.pos).chain(self.current.as_ref().map(|c| c.pos)).min().unwrap_or(self.last_stop).min(have);
        if keep_from > self.base {
            let n = ((keep_from - self.base) as usize).min(self.audio.len());
            self.audio.drain(..n);
            self.base += n as u64;
        }
    }
}

pub struct Recorder {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn spawn(mut ports: RecorderPorts) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("daw-recorder".into())
            .spawn(move || {
                let mut chunk: Vec<f32> = Vec::with_capacity(1 << 16);
                while !sp.load(Ordering::Relaxed) {
                    Self::drain(&mut ports, &st, &mut chunk);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Self::drain(&mut ports, &st, &mut chunk);
            })
            .expect("spawn recorder");
        Self { state, stop, thread: Some(thread) }
    }

    fn drain(ports: &mut RecorderPorts, st: &Mutex<State>, chunk: &mut Vec<f32>) {
        // Audio first: every frame of a take is pushed before its Stopped event.
        chunk.clear();
        let n = ports.audio.slots();
        if let Ok(c) = ports.audio.read_chunk(n) {
            let (a, b) = c.as_slices();
            chunk.extend_from_slice(a);
            chunk.extend_from_slice(b);
            c.commit_all();
        }
        let mut s = st.lock().unwrap();
        s.audio.extend_from_slice(chunk);
        while let Ok(ev) = ports.events.pop() {
            s.on_event(ev);
        }
        s.finish_ready();
    }

    /// Finished takes since the last call.
    pub fn take_finished(&self) -> Vec<Take> {
        std::mem::take(&mut self.state.lock().unwrap().done)
    }

    pub fn is_recording(&self) -> bool {
        self.state.lock().unwrap().recording
    }

    /// Wait until the recorder has nothing in flight (tests/export).
    pub fn flush(&self, timeout: Duration) -> bool {
        let t = std::time::Instant::now();
        while t.elapsed() < timeout {
            if self.state.lock().unwrap().finishing.is_empty() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_engine::{Command, EngineConfig, MIDI_ALL_INS, Monitor, Song, SongTrack, TrackInput, create};

    #[test]
    fn records_audio_and_notes_into_a_take() {
        let (mut engine, mut h) = create(EngineConfig { sample_rate: 48_000.0, max_block_frames: 512, input_channels: 2 });
        let mut input = h.take_input().unwrap();
        let rec = Recorder::spawn(h.take_recorder().unwrap());
        let midi = SongTrack {
            rt: 0,
            arm: true,
            monitor: Monitor::Auto,
            input: TrackInput::Midi { port: MIDI_ALL_INS, channel: 0 },
            ..Default::default()
        };
        let audio = SongTrack { rt: 1, arm: true, input: TrackInput::Ext { ch: 0, stereo: true }, ..Default::default() };
        let mono = SongTrack { rt: 2, arm: true, input: TrackInput::Ext { ch: 1, stereo: false }, ..Default::default() };
        h.set_song(Song::new(vec![midi, audio, mono])).unwrap();
        for c in [Command::SetLaunchQuantize(0.0), Command::SetRecording(true), Command::Play] {
            h.send(c).unwrap();
        }
        let mut out = vec![0.0f32; 1024];
        let mut counter = 0.0f32;
        for i in 0..200 {
            // Feed a ramp so we can verify continuity of the captured audio.
            let mut buf = Vec::with_capacity(1024);
            for _ in 0..512 {
                buf.push(counter);
                buf.push(-counter);
                counter += 1.0;
            }
            if let Ok(chunk) = input.write_chunk_uninit(1024) {
                chunk.fill_from_iter(buf);
            }
            let kb = |pitch, velocity| Command::KeyboardNote { pitch, velocity };
            match i {
                10 => h.send(kb(60, 90)).unwrap(),
                20 => h.send(kb(60, 0)).unwrap(),
                30 => h.send(kb(64, 80)).unwrap(),
                150 => h.send(Command::SetRecording(false)).unwrap(),
                _ => {}
            }
            engine.process(&mut out, 2);
            if i % 7 == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(rec.flush(Duration::from_secs(5)));
        let takes = rec.take_finished();
        assert_eq!(takes.len(), 1);
        let t = &takes[0];
        assert_eq!(t.start_beat, 0.0);
        assert_eq!(t.lanes.len(), 2, "one lane per armed audio track");
        let (st, mo) = (t.lane(1).unwrap(), t.lane(2).unwrap());
        assert!(!st.mono && mo.mono);
        assert_eq!(st.frames(), 150 * 512);
        assert!((t.end_beat - 150.0 * 512.0 / 24_000.0).abs() < 1e-9);
        // Continuous ramp: no dropped or duplicated frames.
        let first = st.audio[0];
        assert!(st.audio.as_chunks::<2>().0.iter().enumerate().all(|(i, f)| f[0] == first + i as f32 && f[1] == -f[0]));
        assert!(mo.audio.as_chunks::<2>().0.iter().all(|f| f[0] == f[1] && f[0] <= 0.0), "mono lane = input 2 on both sides");
        assert_eq!(t.notes.len(), 2);
        assert_eq!((t.notes[0].pitch, t.notes[0].velocity), (60, 90));
        assert!((t.notes[0].start_beat - 10.0 * 512.0 / 24_000.0).abs() < 1e-9);
        assert!((t.notes[0].end_beat - 20.0 * 512.0 / 24_000.0).abs() < 1e-9);
        assert_eq!(t.notes[1].end_beat, t.end_beat, "hanging note closed at stop");
        assert!(!rec.is_recording());
    }

    #[test]
    fn latency_compensation_shifts_audio() {
        let mut t = TakeLane { audio: (0..20).map(|v| v as f32).collect(), ..Default::default() };
        t.compensate_latency(3);
        assert_eq!(t.frames(), 10);
        assert_eq!(&t.audio[..2], &[6.0, 7.0]);
        assert_eq!(&t.audio[18..], &[0.0, 0.0]);
    }
}
