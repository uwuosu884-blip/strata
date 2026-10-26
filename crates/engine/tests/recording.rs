//! Recording, input monitoring.

mod common;
use common::*;
use daw_engine::*;

fn ext_track(rt: u16) -> SongTrack {
    SongTrack { rt, arm: true, input: TrackInput::Ext { ch: 0, stereo: true }, ..Default::default() }
}

fn started(beat: f64, pos: u64, lanes: &[u16], mono: u16) -> RecordEvent {
    let mut l = [u16::MAX; MAX_REC_LANES];
    l[..lanes.len()].copy_from_slice(lanes);
    RecordEvent::Started { beat, pos, lanes: l, n_lanes: lanes.len() as u8, mono }
}

#[test]
fn audio_recording_starts_and_stops_on_bar_boundaries() {
    let (mut e, mut h) = engine();
    let mut input = h.take_input().unwrap();
    let mut rec = h.take_recorder().unwrap();
    h.set_song(Song::new(vec![ext_track(0)])).unwrap();
    h.send(Command::Play).unwrap();
    render(&mut e, SPB, 512); // beat 1
    h.send(Command::SetRecording(true)).unwrap();
    apply(&mut e);
    assert_eq!(h.snapshot().record_state, RecordState::StartPending);

    // Feed input continuously: sample value = running frame counter.
    let mut counter = 0u32;
    let mut got_audio: Vec<f32> = Vec::new();
    let mut step = |e: &mut Engine, frames: usize, input: &mut rtrb::Producer<f32>, rec: &mut RecorderPorts| {
        for _ in 0..frames {
            let v = counter as f32;
            input.push(v).unwrap();
            input.push(-v).unwrap();
            counter += 1;
        }
        render(e, frames, frames);
        while let Ok(s) = rec.audio.pop() {
            got_audio.push(s);
        }
    };
    for _ in 0..(SPB * 4 / 480) {
        step(&mut e, 480, &mut input, &mut rec); // beats 1..5
    }
    assert_eq!(h.snapshot().record_state, RecordState::Recording);
    h.send(Command::SetRecording(false)).unwrap();
    for _ in 0..(SPB * 4 / 480) {
        step(&mut e, 480, &mut input, &mut rec); // beats 5..9
    }
    assert_eq!(h.snapshot().record_state, RecordState::Idle);

    let events: Vec<RecordEvent> = std::iter::from_fn(|| rec.events.pop().ok()).collect();
    assert_eq!(events, vec![started(4.0, 0, &[0], 0), RecordEvent::Stopped { beat: 8.0, pos: 8 * SPB as u64 }]);
    // Exactly 4 beats of stereo audio, starting with the input frame at beat 4.
    assert_eq!(got_audio.len(), 4 * SPB * 2);
    assert_eq!(got_audio[0], (3 * SPB) as f32, "first recorded frame = input at beat 4");
    assert_eq!(got_audio[1], -((3 * SPB) as f32));
}

#[test]
fn record_while_stopped_starts_immediately_and_transport_stop_ends_it() {
    let (mut e, mut h) = engine();
    let mut rec = h.take_recorder().unwrap();
    h.set_song(Song::new(vec![ext_track(0)])).unwrap();
    h.send(Command::SetRecording(true)).unwrap();
    render(&mut e, 1000, 500);
    h.send(Command::Stop).unwrap();
    apply(&mut e);
    let events: Vec<RecordEvent> = std::iter::from_fn(|| rec.events.pop().ok()).collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], started(0.0, 0, &[0], 0));
    assert!(matches!(events[1], RecordEvent::Stopped { beat, pos: 2000 } if (beat - 1000.0 / SPB as f64).abs() < 1e-9));
    assert_eq!(std::iter::from_fn(|| rec.audio.pop().ok()).count(), 2000);
}

#[test]
fn live_notes_are_recorded_with_clip_position() {
    let (mut e, mut h) = engine();
    let mut rec = h.take_recorder().unwrap();
    h.set_song(Song::new(vec![SongTrack { slots: vec![Some(midi_clip(2.0, &[]))], ..midi_track(0, true) }])).unwrap();
    send_all(&mut h, &[Command::LaunchClip { track: 0, scene: 0 }, Command::SetRecording(true)]);
    render(&mut e, SPB * 3, 480); // beat 3 → clip pos 1.0
    h.send(Command::KeyboardNote { pitch: 64, velocity: 90 }).unwrap();
    render(&mut e, SPB / 2, 480);
    h.send(Command::KeyboardNote { pitch: 64, velocity: 0 }).unwrap();
    apply(&mut e);
    let ev: Vec<RecordEvent> = std::iter::from_fn(|| rec.events.pop().ok()).collect();
    assert!(matches!(ev[0], RecordEvent::Started { beat, .. } if beat == 0.0));
    match ev[1] {
        RecordEvent::NoteOn { track: 0, pitch: 64, velocity: 90, beat, clip_pos: Some(pos) } => {
            assert!((beat - 3.0).abs() < 1e-9 && (pos - 1.0).abs() < 1e-9, "{beat} {pos}");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(ev[2], RecordEvent::NoteOff { pitch: 64, clip_pos: Some(p), .. } if (p - 1.5).abs() < 1e-9));
}

#[test]
fn input_monitoring_passes_audio_through() {
    let (mut e, mut h) = engine();
    let mut input = h.take_input().unwrap();
    h.set_song(Song::new(vec![SongTrack { monitor: Monitor::In, ..ext_track(0) }])).unwrap();
    let mut out = Vec::new();
    for _ in 0..10 {
        for _ in 0..480 {
            input.push(0.25).unwrap();
            input.push(0.5).unwrap();
        }
        out.extend(render(&mut e, 480, 480));
    }
    assert!((left(&out)[4000] - 0.25).abs() < 1e-4 && (right(&out)[4000] - 0.5).abs() < 1e-4);
    // Underrun → silence, not garbage.
    let out = render(&mut e, 4800, 480);
    assert!(left(&out)[2000..].iter().all(|&s| s == 0.0));
}
