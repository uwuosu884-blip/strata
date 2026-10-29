//! Routing & I/O: inputs, monitoring, outputs, track-to-track, taps, solo,
//! resampling, MIDI routing and per-track record lanes.

mod common;
use common::*;
use daw_engine::*;

/// Engine with a 4-channel input; ch k carries the constant (k + 1) / 10.
fn engine4() -> (Engine, EngineHandle, rtrb::Producer<f32>) {
    let (e, mut h) = create(EngineConfig { sample_rate: SR, max_block_frames: 512, input_channels: 4 });
    h.send(Command::SetMetronome(false)).unwrap();
    let input = h.take_input().unwrap();
    (e, h, input)
}

/// Render `frames` frames (cb 480) on `channels` outputs while feeding input.
fn run(e: &mut Engine, input: &mut rtrb::Producer<f32>, frames: usize, channels: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * channels];
    for chunk in out.chunks_mut(480 * channels) {
        for _ in 0..chunk.len() / channels {
            for c in 0..4 {
                let _ = input.push((c + 1) as f32 / 10.0);
            }
        }
        e.process(chunk, channels);
    }
    out
}

fn ch(v: &[f32], channels: usize, c: usize) -> f32 {
    v[v.len() - channels + c] // last frame
}

fn ext(rt: u16, ch: u16, stereo: bool, monitor: Monitor) -> SongTrack {
    SongTrack { rt, input: TrackInput::Ext { ch, stereo }, monitor, ..Default::default() }
}

#[test]
fn ext_input_channels_mono_and_stereo() {
    let (mut e, mut h, mut input) = engine4();
    h.set_song(Song::new(vec![ext(0, 2, true, Monitor::In)])).unwrap();
    let out = run(&mut e, &mut input, 4800, 2);
    assert!((ch(&out, 2, 0) - 0.3).abs() < 1e-5 && (ch(&out, 2, 1) - 0.4).abs() < 1e-5, "stereo 3/4");
    h.set_song(Song::new(vec![ext(0, 1, false, Monitor::In)])).unwrap();
    let out = run(&mut e, &mut input, 4800, 2);
    assert!((ch(&out, 2, 0) - 0.2).abs() < 1e-5 && (ch(&out, 2, 1) - 0.2).abs() < 1e-5, "mono 2 → both sides");
    let p = h.input_peaks();
    assert!((p[0] - 0.1).abs() < 1e-6 && (p[3] - 0.4).abs() < 1e-6 && p[4] == 0.0, "per-channel input meters {p:?}");
}

#[test]
fn monitor_in_auto_off() {
    let (mut e, mut h, mut input) = engine4();
    let clip = dc_clip(0.5);
    let song = |monitor, arm| Song::new(vec![SongTrack { arm, slots: vec![Some(clip.clone())], ..ext(0, 0, false, monitor) }]);
    let level = |e: &mut Engine, input: &mut rtrb::Producer<f32>| ch(&run(e, input, 4800, 2), 2, 0);
    h.set_song(song(Monitor::Off, true)).unwrap();
    assert_eq!(level(&mut e, &mut input), 0.0, "Off");
    h.set_song(song(Monitor::Auto, false)).unwrap();
    assert_eq!(level(&mut e, &mut input), 0.0, "Auto unarmed");
    h.set_song(song(Monitor::Auto, true)).unwrap();
    assert!((level(&mut e, &mut input) - 0.1).abs() < 1e-5, "Auto armed");
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    assert!((level(&mut e, &mut input) - 0.5).abs() < 1e-4, "Auto: clip playing → input off");
    h.set_song(song(Monitor::In, false)).unwrap();
    assert!((level(&mut e, &mut input) - 0.1).abs() < 1e-5, "In: input only, clip suppressed");
}

#[test]
fn outputs_ext_mono_stereo_and_master_pair() {
    let (mut e, mut h, mut input) = engine4();
    let mut s = Song::new(vec![
        SongTrack { output: TrackOutput::Ext { ch: 2, stereo: false }, ..ext(0, 0, true, Monitor::In) },
        SongTrack { output: TrackOutput::Ext { ch: 4, stereo: true }, ..ext(1, 2, true, Monitor::In) },
        ext(2, 3, false, Monitor::In),
    ]);
    s.master_out = (0, true);
    h.set_song(s.clone()).unwrap();
    let out = run(&mut e, &mut input, 4800, 6);
    let got: Vec<f32> = (0..6).map(|c| (ch(&out, 6, c) * 1e4).round() / 1e4).collect();
    assert_eq!(got, vec![0.4, 0.4, 0.15, 0.0, 0.3, 0.4], "mono out = (L+R)/2");
    s.master_out = (2, false);
    h.set_song(s).unwrap();
    let out = run(&mut e, &mut input, 4800, 6);
    assert!((ch(&out, 6, 2) - (0.4 + 0.15)).abs() < 1e-4 && ch(&out, 6, 0) == 0.0, "master to mono 3");
}

#[test]
fn track_to_track_many_to_one_and_solo() {
    let (mut e, mut h) = engine();
    let to_b = |rt, v| SongTrack { output: TrackOutput::Track(9), ..audio_track(rt, dc_clip(v)) };
    let b = SongTrack { rt: 9, input: TrackInput::None, ..audio_track(9, dc_clip(0.0625)) };
    let c = audio_track(3, dc_clip(0.03125));
    h.set_song(Song::new(vec![to_b(0, 0.5), to_b(1, 0.25), b, c])).unwrap();
    h.send(Command::LaunchScene { scene: 0 }).unwrap();
    let level = |e: &mut Engine| *left(&render(e, 4800, 480)).last().unwrap();
    assert!((level(&mut e) - (0.5 + 0.25 + 0.0625 + 0.03125)).abs() < 1e-4);
    h.send(Command::SetTrackGain { track: 9, gain: 0.5 }).unwrap();
    assert!((level(&mut e) - ((0.5 + 0.25 + 0.0625) * 0.5 + 0.03125)).abs() < 1e-4, "feeders go through B's fader");
    h.send(Command::SetTrackGain { track: 9, gain: 1.0 }).unwrap();
    h.send(Command::SetTrackSolo { track: 9, solo: true }).unwrap();
    assert!((level(&mut e) - (0.5 + 0.25 + 0.0625)).abs() < 1e-4, "solo B hears its feeders");
    send_all(&mut h, &[Command::SetTrackSolo { track: 9, solo: false }, Command::SetTrackSolo { track: 0, solo: true }]);
    assert!((level(&mut e) - (0.5 + 0.0625)).abs() < 1e-4, "solo A: A through B, other feeder muted");
}

#[test]
fn input_from_track_taps_and_solo() {
    let (mut e, mut h) = engine();
    let a = audio_track(0, dc_clip(0.5));
    let tap = |tap| SongTrack { rt: 1, input: TrackInput::Track { rt: 0, tap }, monitor: Monitor::In, ..Default::default() };
    for (t, solo_hears) in [(Tap::PreFx, 0.5), (Tap::PostFx, 0.5), (Tap::PostMixer, 0.0)] {
        h.set_song(Song::new(vec![a.clone(), tap(t)])).unwrap();
        send_all(&mut h, &[Command::SetTrackSolo { track: 1, solo: false }, Command::SetTrackGain { track: 0, gain: 0.5 }]);
        h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
        let level = |e: &mut Engine| *left(&render(e, 9600, 480)).last().unwrap();
        let both = level(&mut e);
        let expect = if t == Tap::PostMixer { 0.25 + 0.25 } else { 0.25 + 0.5 };
        assert!((both - expect).abs() < 1e-4, "{t:?}: {both}");
        h.send(Command::SetTrackSolo { track: 1, solo: true }).unwrap();
        assert!((level(&mut e) - solo_hears).abs() < 1e-4, "{t:?} soloed tapper");
        send_all(&mut h, &[Command::Stop, Command::Rewind]);
        render(&mut e, 48000, 480);
    }
}

#[test]
fn drum_pad_output_is_removed_from_the_drum_mix() {
    let (mut e, mut h) = engine();
    let s = impulse();
    let pads = (0..DRUM_PADS)
        .map(|_| DrumPad { name: "p".into(), sample: Some(s.clone()), gain: 1.0, tune_semitones: 0.0, choke: 0 })
        .collect::<Vec<_>>();
    let drums = SongTrack { rt: 0, instrument: Instrument::Drums(pads), ..Default::default() };
    let tapper =
        SongTrack { rt: 1, input: TrackInput::Track { rt: 0, tap: Tap::Pad(1) }, monitor: Monitor::In, pan: -1.0, ..Default::default() };
    h.set_song(Song::new(vec![drums.clone(), tapper])).unwrap();
    apply(&mut e);
    let hit = |e: &mut Engine, h: &mut EngineHandle, pad: u8| {
        h.send(Command::NoteOn { track: 0, pitch: DRUM_BASE_NOTE + pad, velocity: 127 }).unwrap();
        let out = render(e, 480, 480);
        render(e, 4800, 480);
        (left(&out)[0], right(&out)[0])
    };
    let (l0, r0) = hit(&mut e, &mut h, 0);
    assert!(l0 > 0.0 && (l0 - r0).abs() < 1e-6, "pad 0 stays in the drum track (centre)");
    let (l1, r1) = hit(&mut e, &mut h, 1);
    assert!(l1 > 0.0 && r1.abs() < 1e-6, "pad 1 only via the hard-left tapper: {l1} {r1}");
}

#[test]
fn resampling_is_heard_without_feedback() {
    let (mut e, mut h) = engine();
    let mut rec = h.take_recorder().unwrap();
    let src = audio_track(0, dc_clip(0.25));
    let res = SongTrack { rt: 1, arm: true, input: TrackInput::Resampling, monitor: Monitor::In, ..audio_track(1, dc_clip(0.5)) };
    h.set_song(Song::new(vec![src, res])).unwrap();
    send_all(&mut h, &[Command::SetLaunchQuantize(0.0), Command::LaunchScene { scene: 0 }, Command::SetRecording(true)]);
    let out = render(&mut e, 9600, 480);
    // Monitored: main = source 0.25 + the resampled 0.25; its own output is
    // not fed back into what it records (would grow without bound).
    assert!((left(&out)[9000] - 0.5).abs() < 1e-4, "resampling track is audible: {}", left(&out)[9000]);
    match rec.events.pop().unwrap() {
        RecordEvent::Started { n_lanes: 1, lanes, mono: 0, .. } => assert_eq!(lanes[0], 1),
        other => panic!("{other:?}"),
    }
    let audio: Vec<f32> = std::iter::from_fn(|| rec.audio.pop().ok()).collect();
    assert!((audio[audio.len() - 2] - 0.25).abs() < 1e-4, "recorded the main output");
}

#[test]
fn record_lanes_per_armed_track() {
    let (mut e, mut h, mut input) = engine4();
    let mut rec = h.take_recorder().unwrap();
    let a = SongTrack { arm: true, ..ext(0, 1, false, Monitor::Off) };
    let b = SongTrack { arm: true, ..ext(5, 2, true, Monitor::Off) };
    let c = SongTrack { arm: false, ..ext(6, 0, true, Monitor::Off) };
    let m = midi_track(7, true);
    h.set_song(Song::new(vec![a, b, c, m])).unwrap();
    h.send(Command::SetRecording(true)).unwrap();
    run(&mut e, &mut input, 960, 2);
    h.send(Command::Stop).unwrap();
    apply(&mut e);
    match rec.events.pop().unwrap() {
        RecordEvent::Started { n_lanes: 2, lanes, mono: 1, .. } => assert_eq!(&lanes[..2], &[0, 5]),
        other => panic!("{other:?}"),
    }
    assert!(matches!(rec.events.pop().unwrap(), RecordEvent::Stopped { pos: 3840, .. }));
    let audio: Vec<f32> = std::iter::from_fn(|| rec.audio.pop().ok()).collect();
    assert_eq!(audio.len(), 960 * 4);
    let r = |v: f32| (v * 1e5).round() / 1e5;
    assert_eq!(audio[1000..1004].iter().map(|&v| r(v)).collect::<Vec<_>>(), vec![0.2, 0.2, 0.3, 0.4]);
}

#[test]
fn midi_routing_between_tracks_and_to_ports() {
    let (mut e, mut h) = engine();
    let mut out = h.take_midi_output().unwrap();
    // Track 0 (no instrument) plays a clip → port 2 ch 5 and → track 1's instrument.
    let src = |output| SongTrack { rt: 0, output, slots: vec![Some(midi_clip(1.0, &[0.0]))], ..Default::default() };
    h.set_song(Song::new(vec![src(TrackOutput::MidiPort { port: 2, channel: 5 })])).unwrap();
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    render(&mut e, SPB / 2, 480);
    assert_eq!(out.pop().unwrap(), MidiOutput { port: 2, bytes: [0x94, 60, 127] });
    render(&mut e, SPB / 4, 480);
    assert_eq!(out.pop().unwrap(), MidiOutput { port: 2, bytes: [0x84, 60, 0] }, "timed note-off (length 0.25)");
    send_all(&mut h, &[Command::Stop, Command::Rewind]);
    render(&mut e, 480, 480);

    // Direct to instrument: heard even though track 1 is not monitoring.
    let dst = SongTrack { monitor: Monitor::Off, ..track(1, vec![]) };
    for (instrument, heard) in [(true, true), (false, false)] {
        h.set_song(Song::new(vec![src(TrackOutput::MidiTrack { rt: 1, instrument }), dst.clone()])).unwrap();
        h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
        let o = left(&render(&mut e, 4800, 480));
        assert_eq!(!nonzero(&o).is_empty(), heard, "instrument={instrument}");
        send_all(&mut h, &[Command::Stop, Command::Rewind]);
        render(&mut e, 4800, 480);
    }
    // Track In + monitor In → heard.
    let dst_in = SongTrack { monitor: Monitor::In, ..dst.clone() };
    h.set_song(Song::new(vec![src(TrackOutput::MidiTrack { rt: 1, instrument: false }), dst_in])).unwrap();
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    assert!(!nonzero(&left(&render(&mut e, 4800, 480))).is_empty());
}

#[test]
fn midi_input_from_another_track_layers_live_notes() {
    let (mut e, mut h) = engine();
    let a = midi_track(0, true);
    let b = SongTrack { rt: 1, input: TrackInput::MidiTrack { rt: 0 }, monitor: Monitor::In, pan: 1.0, ..track(1, vec![]) };
    let a = SongTrack { pan: -1.0, ..a };
    h.set_song(Song::new(vec![b, a])).unwrap();
    h.send(Command::KeyboardNote { pitch: 60, velocity: 100 }).unwrap();
    let out = render(&mut e, 480, 480);
    assert!(left(&out)[0] > 0.0 && right(&out)[0] > 0.0, "both layers sound");
}

#[test]
fn keyboard_notes_reach_keyboard_and_all_ins_tracks_only() {
    let (mut e, mut h) = engine();
    let only_port = SongTrack { input: TrackInput::Midi { port: 0, channel: 0 }, ..midi_track(0, true) };
    h.set_song(Song::new(vec![only_port])).unwrap();
    h.send(Command::KeyboardNote { pitch: 60, velocity: 100 }).unwrap();
    assert!(nonzero(&render(&mut e, 480, 480)).is_empty());
    let kb = SongTrack { input: TrackInput::Midi { port: MIDI_KEYBOARD, channel: 0 }, ..midi_track(0, true) };
    h.set_song(Song::new(vec![kb])).unwrap();
    h.send(Command::KeyboardNote { pitch: 61, velocity: 100 }).unwrap();
    assert!(!nonzero(&render(&mut e, 480, 480)).is_empty());
}

#[test]
fn post_fader_sends_feed_return_tracks() {
    let (mut e, mut h, mut input) = engine4();
    // Track 0 hears input ch 1 (0.1) and only sends (output None) at 0.5 to return rt 1.
    let song = |send: f32, gain: f32, mute: bool, solo: (bool, bool)| {
        Song::new(vec![
            SongTrack { output: TrackOutput::None, sends: vec![(1, send)], gain, mute, solo: solo.0, ..ext(0, 0, false, Monitor::In) },
            SongTrack { rt: 1, ..Default::default() },
            SongTrack { rt: 2, solo: solo.1, ..Default::default() },
        ])
    };
    let level = |e: &mut Engine, input: &mut rtrb::Producer<f32>| ch(&run(e, input, 4800, 2), 2, 0);
    h.set_song(song(0.5, 1.0, false, (false, false))).unwrap();
    assert!((level(&mut e, &mut input) - 0.05).abs() < 1e-4, "send 0.5");
    h.set_song(song(0.5, 0.5, false, (false, false))).unwrap();
    assert!((level(&mut e, &mut input) - 0.025).abs() < 1e-4, "post-fader");
    h.set_song(song(0.5, 1.0, true, (false, false))).unwrap();
    assert!(level(&mut e, &mut input).abs() < 1e-4, "muted track sends nothing");
    h.set_song(song(0.5, 1.0, false, (true, false))).unwrap();
    assert!((level(&mut e, &mut input) - 0.05).abs() < 1e-4, "solo in place keeps the return");
    h.set_song(song(0.5, 1.0, false, (false, true))).unwrap();
    assert!(level(&mut e, &mut input).abs() < 1e-4, "another solo silences track + return");
    // Self / dangling sends are dropped; cycles via sends are cut.
    let mut s = Song::new(vec![SongTrack { sends: vec![(0, 1.0), (9, 1.0)], ..Default::default() }]);
    s.prepare();
    assert!(s.tracks[0].sends.is_empty());
}

#[test]
fn soloed_return_keeps_its_senders_and_set_send_retunes() {
    let (mut e, mut h, mut input) = engine4();
    // Track 0 (input 0.1) → Master and a 0.5 send to return rt 1 (→ Master).
    let song = |solo_ret: bool| {
        Song::new(vec![
            SongTrack { output: TrackOutput::Master, sends: vec![(1, 0.5)], ..ext(0, 0, false, Monitor::In) },
            SongTrack { rt: 1, output: TrackOutput::Master, solo: solo_ret, ..Default::default() },
        ])
    };
    let level = |e: &mut Engine, input: &mut rtrb::Producer<f32>| ch(&run(e, input, 4800, 2), 2, 0);
    h.set_song(song(false)).unwrap();
    let both = level(&mut e, &mut input);
    h.set_song(song(true)).unwrap();
    let ret_only = level(&mut e, &mut input);
    assert!(both > ret_only + 0.01, "direct path silenced: {both} vs {ret_only}");
    assert!(ret_only > 0.01, "the soloed return still gets the send");
    // SetSend changes the level of an existing send without a new song.
    h.send(Command::SetSend { track: 0, ret: 1, gain: 0.0 }).unwrap();
    assert!(level(&mut e, &mut input).abs() < 1e-4, "send at 0 → return silent");
}
