//! Session view (clip launching) and arrangement playback timing.

mod common;
use common::*;
use daw_engine::*;

#[test]
fn launch_is_quantized_to_next_bar() {
    let (mut e, mut h) = engine();
    h.set_song(Song::new(vec![track(0, vec![Some(midi_clip(4.0, &[0.0, 1.0, 2.0, 3.0]))])])).unwrap();
    h.send(Command::Play).unwrap();
    render(&mut e, SPB * 3 / 2, 512); // now at beat 1.5
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 7, 512)); // beats 1.5 .. 8.5
    let got: Vec<f64> = beats(&nonzero(&out)).iter().map(|b| b + 1.5).collect();
    assert_eq!(got, vec![4.0, 5.0, 6.0, 7.0, 8.0]);
}

#[test]
fn launch_while_stopped_starts_transport_immediately() {
    let (mut e, mut h) = engine();
    h.set_song(Song::new(vec![track(0, vec![Some(midi_clip(1.0, &[0.0]))])])).unwrap();
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 3 + 10, 512));
    assert_eq!(nonzero(&out), vec![0, SPB, 2 * SPB, 3 * SPB]);
    assert!(h.snapshot().playing);
}

#[test]
fn looping_is_exact_for_any_callback_size() {
    let run = |cb: usize| {
        let (mut e, mut h) = engine();
        h.set_song(Song::new(vec![track(0, vec![Some(midi_clip(1.5, &[0.0, 0.5, 1.25]))])])).unwrap();
        send_all(&mut h, &[Command::SetTempo(137.0), Command::LaunchClip { track: 0, scene: 0 }]);
        nonzero(&left(&render(&mut e, 400_000, cb)))
    };
    let reference = run(512);
    assert!(reference.len() > 30, "{}", reference.len()); // ≈19 beats → 12 loops × 3 notes
    for cb in [1, 7, 333, 4096] {
        assert_eq!(run(cb), reference, "callback size {cb}");
    }
}

#[test]
fn stop_track_is_quantized() {
    let (mut e, mut h) = engine();
    h.set_song(Song::new(vec![track(0, vec![Some(midi_clip(1.0, &[0.0]))])])).unwrap();
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    render(&mut e, SPB * 9 / 2, 512); // beat 4.5
    h.send(Command::StopTrack { track: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 6, 512));
    let got: Vec<f64> = beats(&nonzero(&out)).iter().map(|b| b + 4.5).collect();
    assert_eq!(got, vec![5.0, 6.0, 7.0], "stops at bar 3 (beat 8)");
    assert_eq!(h.track(0).playing_scene, None);
}

#[test]
fn scene_launch_switches_and_stops_empty_tracks() {
    let (mut e, mut h) = engine();
    let a = track(0, vec![Some(midi_clip(1.0, &[0.0])), Some(midi_clip(1.0, &[0.5]))]);
    let b = track(1, vec![Some(midi_clip(1.0, &[0.0])), None]);
    h.set_song(Song::new(vec![a, b])).unwrap();
    h.send(Command::LaunchScene { scene: 0 }).unwrap();
    render(&mut e, SPB * 2, 512);
    assert_eq!(h.track(0).playing_scene, Some(0));
    assert_eq!(h.track(1).playing_scene, Some(0));
    h.send(Command::LaunchScene { scene: 1 }).unwrap();
    apply(&mut e);
    assert_eq!(h.track(0).pending, Some(PendingLaunch::Scene(1)));
    assert_eq!(h.track(1).pending, Some(PendingLaunch::Stop));
    let out = left(&render(&mut e, SPB * 4, 512)); // beats 2..6
    let got: Vec<f64> = beats(&nonzero(&out)).iter().map(|b| b + 2.0).collect();
    // beats 2,3 from both tracks (summed into one sample each), then scene 1: offbeats only.
    assert_eq!(got, vec![2.0, 3.0, 4.5, 5.5]);
    assert_eq!(h.track(1).playing_scene, None);
}

#[test]
fn non_looping_clip_stops_at_its_end() {
    let (mut e, mut h) = engine();
    let mut clip = midi_clip(2.0, &[0.0, 1.0]);
    clip.looping = false;
    h.set_song(Song::new(vec![track(0, vec![Some(clip)])])).unwrap();
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    let out = left(&render(&mut e, SPB * 5, 512));
    assert_eq!(nonzero(&out), vec![0, SPB]);
    assert_eq!(h.track(0).playing_scene, None);
}

#[test]
fn arrangement_plays_and_session_overrides_until_back_to_arrangement() {
    let (mut e, mut h) = engine();
    let mut t = track(0, vec![Some(midi_clip(1.0, &[0.5]))]);
    t.arrangement = vec![ArrangementClip { start: 2.0, clip: midi_clip(4.0, &[0.0, 1.0, 2.0, 3.0]) }];
    h.set_song(Song::new(vec![t])).unwrap();
    send_all(&mut h, &[Command::SetLaunchQuantize(1.0), Command::Play]);
    let out = left(&render(&mut e, SPB * 4, 512));
    assert_eq!(beats(&nonzero(&out)), vec![2.0, 3.0], "arrangement clip starts at beat 2");

    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap(); // at beat 4
    let out = left(&render(&mut e, SPB * 2, 512)); // beats 4..6
    assert_eq!(beats(&nonzero(&out)), vec![0.5, 1.5], "session clip replaces arrangement");
    assert!(h.track(0).arrangement_off);

    h.send(Command::BackToArrangement).unwrap();
    h.send(Command::SetPosition(2.0)).unwrap();
    let out = left(&render(&mut e, SPB * 2, 512));
    assert_eq!(beats(&nonzero(&out)), vec![0.0, 1.0]);
    assert!(!h.track(0).arrangement_off);
}

#[test]
fn arrangement_loop_region_repeats() {
    let (mut e, mut h) = engine();
    let mut t = track(0, vec![]);
    t.arrangement = vec![ArrangementClip { start: 0.0, clip: midi_clip(8.0, &[1.0, 3.0]) }];
    h.set_song(Song::new(vec![t])).unwrap();
    send_all(&mut h, &[Command::SetLoop { enabled: true, start: 0.0, end: 2.0 }, Command::Play]);
    let out = left(&render(&mut e, SPB * 6 + 5, 333));
    assert_eq!(beats(&nonzero(&out)), vec![1.0, 3.0, 5.0], "note at beat 3 never reached");
    assert!(h.snapshot().beat_position < 2.0);
}

#[test]
fn set_position_keeps_session_clip_phase() {
    let (mut e, mut h) = engine();
    h.set_song(Song::new(vec![track(0, vec![Some(midi_clip(1.0, &[0.0]))])])).unwrap();
    h.send(Command::LaunchClip { track: 0, scene: 0 }).unwrap();
    render(&mut e, SPB / 4, 512);
    h.send(Command::SetPosition(10.0)).unwrap();
    let out = left(&render(&mut e, SPB, 512));
    assert_eq!(nonzero(&out), vec![SPB * 3 / 4]);
}

#[test]
fn removing_a_track_from_the_song_silences_it() {
    let (mut e, mut h) = engine();
    h.set_song(Song::new(vec![track(5, vec![Some(midi_clip(1.0, &[0.0]))])])).unwrap();
    h.send(Command::LaunchClip { track: 5, scene: 0 }).unwrap();
    render(&mut e, SPB / 2, 512);
    h.set_song(Song::default()).unwrap();
    let out = render(&mut e, SPB * 2, 512);
    assert!(nonzero(&out).is_empty());
    // set_song() already freed the first old song; the second comes back now.
    assert_eq!(h.collect_garbage(), 1, "old songs come back for deallocation");
}

/// Loop start on a clip's first note with a loop length that isn't a whole
/// number of samples: the wrap must not skip that note on later passes, nor
/// fire a note sitting exactly on the loop end.
#[test]
fn arrangement_loop_replays_note_on_loop_start() {
    let (mut e, mut h) = engine();
    let mut t = track(0, vec![]);
    let (a, b) = (1.0, 2.000_013);
    t.arrangement = vec![ArrangementClip { start: a, clip: midi_clip(4.0, &[0.0, b - a]) }];
    h.set_song(Song::new(vec![t])).unwrap();
    send_all(&mut h, &[Command::SetLoop { enabled: true, start: a, end: b }, Command::SetPosition(a), Command::Play]);
    let out = left(&render(&mut e, SPB * 5, 256));
    // 5 beats of a ~1 beat loop: 5 passes (the last one starts at ~4.99996).
    assert_eq!(nonzero(&out).len(), 5, "first note must play on every pass: {:?}", nonzero(&out));
}
