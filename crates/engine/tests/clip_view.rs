//! Clip View playback (start / loop markers), launch modes, follow actions,
//! legato, tempo map (leader) and warp modes.

mod common;
use common::*;
use daw_engine::*;
use std::sync::Arc;

fn started(e: &mut Engine, h: &mut EngineHandle, song: Song) {
    h.set_song(song).unwrap();
    send_all(h, &[Command::SetLaunchQuantize(0.0)]);
    let _ = e;
}

#[test]
fn start_marker_then_loop_region() {
    let (mut e, mut h) = engine();
    // Notes at 0,1,2,3; start at 1, loop 2..4 → plays 1,2,3, then 2,3,2,3…
    let mut c = midi_clip(4.0, &[0.0, 1.0, 2.0, 3.0]);
    c.start = 1.0;
    c.loop_start = 2.0;
    c.loop_end = 4.0;
    started(&mut e, &mut h, Song::new(vec![track(0, vec![Some(c)])]));
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 7 - 10, 480));
    assert_eq!(beats(&nonzero(&out)), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    // Content positions reported: 1, 2, 3, then 2, 3 …
    let pos = h.track(0).clip_pos;
    assert!((2.0..4.0).contains(&pos), "{pos}");
}

#[test]
fn unlooped_clip_plays_from_start_for_length() {
    let (mut e, mut h) = engine();
    let mut c = midi_clip(2.0, &[0.0, 1.0, 2.0, 3.0]);
    c.looping = false;
    c.start = 2.0; // plays content 2..4
    started(&mut e, &mut h, Song::new(vec![track(0, vec![Some(c)])]));
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 5, 480));
    assert_eq!(beats(&nonzero(&out)), vec![0.0, 1.0]);
}

#[test]
fn toggle_mode_stops_on_second_press_and_gate_on_release() {
    let (mut e, mut h) = engine();
    let mut t = midi_clip(1.0, &[0.0]);
    t.extra.launch.mode = LaunchMode::Toggle;
    let mut g = midi_clip(1.0, &[0.0]);
    g.extra.launch.mode = LaunchMode::Gate;
    started(&mut e, &mut h, Song::new(vec![track(0, vec![Some(t), Some(g)])]));
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    render(&mut e, SPB * 2, 480);
    assert_eq!(h.track(0).playing_scene, Some(0));
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    render(&mut e, 480, 480);
    assert_eq!(h.track(0).playing_scene, None, "toggle stops");
    h.send(Command::LaunchClip { track: 0, scene: 1 }).unwrap();
    render(&mut e, SPB, 480);
    assert_eq!(h.track(0).playing_scene, Some(1));
    h.send(Command::ReleaseClip { track: 0, scene: 1 }).unwrap();
    render(&mut e, 480, 480);
    assert_eq!(h.track(0).playing_scene, None, "gate stops on release");
}

#[test]
fn repeat_mode_retriggers_while_held() {
    let (mut e, mut h) = engine();
    let mut c = midi_clip(4.0, &[0.0]); // one note per 4-beat loop
    c.extra.launch.mode = LaunchMode::Repeat;
    c.extra.launch.quantize = Some(1.0);
    started(&mut e, &mut h, Song::new(vec![track(0, vec![Some(c)])]));
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 3 + 10, 480));
    assert_eq!(beats(&nonzero(&out)), vec![0.0, 1.0, 2.0, 3.0], "retriggered every beat");
    h.send(Command::ReleaseClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 2, 480));
    assert!(nonzero(&out).is_empty(), "released: plays on without retriggering");
}

#[test]
fn follow_action_next_after_time() {
    let (mut e, mut h) = engine();
    let mut a = midi_clip(1.0, &[0.0]);
    a.extra.launch.follow = Some((Follow::Next, Follow::None, 1.0, 2.0));
    let b = midi_clip(1.0, &[0.0, 0.5]);
    started(&mut e, &mut h, Song::new(vec![track(0, vec![Some(a), None, Some(b)])]));
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 3 + 10, 480));
    assert_eq!(beats(&nonzero(&out)), vec![0.0, 1.0, 2.0, 2.5, 3.0], "next skips the empty slot");
    assert_eq!(h.track(0).playing_scene, Some(2));
}

#[test]
fn legato_keeps_play_position() {
    let (mut e, mut h) = engine();
    let a = midi_clip(4.0, &[0.0]);
    let mut b = midi_clip(4.0, &[0.0, 2.0]);
    b.extra.launch.legato = true;
    started(&mut e, &mut h, Song::new(vec![track(0, vec![Some(a), Some(b)])]));
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    render(&mut e, SPB + SPB / 2, 480); // beat 1.5
    h.send(Command::LaunchClip { track: 0, scene: 1 }).unwrap();
    let out = left(&render(&mut e, SPB * 2, 480)); // 1.5 .. 3.5
    assert_eq!(beats(&nonzero(&out)).iter().map(|b| b + 1.5).collect::<Vec<_>>(), vec![2.0], "b continues at a's position");
}

#[test]
fn tempo_map_steps_and_unfollow() {
    let (mut e, mut h) = engine();
    let mut song = Song::new(vec![track(0, vec![])]);
    song.tempo_map = vec![(1.0, 60.0)];
    song.base_tempo = 120.0;
    h.set_song(song).unwrap();
    h.send(Command::Play).unwrap();
    render(&mut e, SPB, 480); // 1 beat at 120
    render(&mut e, 4800, 480);
    assert_eq!(h.snapshot().tempo, 60.0);
    let b = h.snapshot().beat_position;
    render(&mut e, 48_000, 480); // 1 s at 60 BPM = 1 beat
    assert!((h.snapshot().beat_position - b - 1.0).abs() < 1e-3);
    send_all(&mut h, &[Command::SetTempoFollow(false), Command::SetTempo(120.0)]);
    render(&mut e, 480, 480);
    assert_eq!(h.snapshot().tempo, 120.0);
}

#[test]
fn nudge_speeds_up_playback() {
    let (mut e, mut h) = engine();
    h.set_song(Song::new(vec![])).unwrap();
    send_all(&mut h, &[Command::Play, Command::Nudge(1.1)]);
    render(&mut e, 48_000, 480);
    assert!((h.snapshot().beat_position - 2.2).abs() < 1e-3);
}

fn sine_clip(mode: StretchMode, bpm_song_ratio: f64) -> Clip {
    // 1 s of 440 Hz at 48 kHz = 2 beats at the sample's 120 BPM.
    let s = Arc::new(SampleBuffer::mono(
        "s",
        48_000,
        (0..96_000).map(|i| (i as f64 / SR * 440.0 * std::f64::consts::TAU).sin() as f32 * 0.5).collect(),
    ));
    let warp = WarpSetup { map: WarpMap::linear(0.0, 24_000.0), mode, segs: vec![0.0, 1.0, 2.0, 3.0], ..Default::default() };
    let _ = bpm_song_ratio;
    Clip::new(4.0, false, ClipContent::Audio(AudioClip { sample: s, offset_frames: 0.0, warp_bpm: Some(120.0), gain: 1.0, warp }))
}

#[test]
fn warp_modes_keep_or_change_pitch() {
    for (mode, expect) in [
        (StretchMode::RePitch, 220.0),
        (StretchMode::Tones, 440.0),
        (StretchMode::Texture, 440.0),
        (StretchMode::Complex, 440.0),
        (StretchMode::ComplexPro, 440.0),
        (StretchMode::Beats, 440.0),
    ] {
        let (mut e, mut h) = engine();
        let t = SongTrack {
            arrangement: vec![ArrangementClip { start: 0.0, clip: sine_clip(mode, 0.5) }],
            ..audio_track(0, midi_clip(1.0, &[]))
        };
        h.set_song(Song::new(vec![t])).unwrap();
        send_all(&mut h, &[Command::SetTempo(60.0), Command::Play]); // half speed
        let out = left(&render(&mut e, 96_000, 480));
        let f = freq(&out[12_000..84_000], SR);
        assert!((f - expect).abs() < expect * 0.04, "{mode:?}: {f} Hz, want {expect}");
    }
}
